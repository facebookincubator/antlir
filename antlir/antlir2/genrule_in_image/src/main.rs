/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use antlir2_overlayfs::OverlayFs;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    layer: PathBuf,
    #[clap(long)]
    rootless: bool,
    #[clap(long)]
    ephemeral: bool,
    #[clap(value_enum, long)]
    /// On-disk format of the layer storage
    working_format: WorkingFormat,
    #[clap(flatten)]
    out: Out,
    #[clap(last(true))]
    command: String,
}

#[derive(Debug, Parser)]
struct Out {
    #[clap(long)]
    out: PathBuf,

    #[clap(long)]
    dir: bool,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
enum WorkingFormat {
    Btrfs,
    Overlayfs,
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let rootless = match args.rootless {
        true => None,
        false => Some(antlir2_rootless::init().context("while setting up antlir2_rootless")?),
    };
    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    let root_guard = rootless.map(|r| r.escalate()).transpose()?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;
    drop(root_guard);

    let overlayfs = match args.working_format {
        WorkingFormat::Overlayfs => {
            let model = std::fs::read_to_string(&args.layer).context("while reading model json")?;
            let model = serde_json::from_str(&model).context("while parsing model json")?;
            let opts = antlir2_overlayfs::Opts::builder().model(model).build();
            let fs = OverlayFs::mount(opts).context("while mounting overlayfs")?;
            Some(fs)
        }
        WorkingFormat::Btrfs => None,
    };

    let mut builder = IsolationContext::builder(
        overlayfs
            .as_ref()
            .map_or(args.layer.as_path(), |o| o.mountpoint()),
    );
    builder.ephemeral(args.ephemeral);
    #[cfg(facebook)]
    builder.platform(["/usr/local/fbcode", "/mnt/gvfs"]);
    let cwd = std::env::current_dir()?;
    builder
        .inputs((
            Path::new("/__genrule_in_image__/working_directory"),
            cwd.as_path(),
        ))
        // TODO(T185979228) we really should make all submounts recursively
        // readonly, but that's hard and for now we should at least make sure
        // that buck-out is readonly
        .inputs((
            Path::new("/__genrule_in_image__/working_directory/buck-out"),
            cwd.join("buck-out"),
        ))
        .working_directory(Path::new("/__genrule_in_image__/working_directory"))
        .tmpfs(Path::new("/tmp"))
        // TODO(vmagro): make this a devtmpfs after resolving permissions issues
        .tmpfs(Path::new("/dev"));

    if args.out.dir {
        std::fs::create_dir_all(&args.out.out)?;
        builder
            .outputs((
                Path::new("/__genrule_in_image__/out"),
                args.out.out.as_path(),
            ))
            .setenv(("OUT", "/__genrule_in_image__/out"));
    } else {
        std::fs::File::create(&args.out.out)?;
        builder
            // some tools might uncontrollably attempt to put temporary files
            // next to the output, so make it a tmpfs
            .tmpfs(Path::new("/__genrule_in_image__/out"))
            .outputs((
                Path::new("/__genrule_in_image__/out/single_file"),
                args.out.out.as_path(),
            ))
            .setenv(("OUT", "/__genrule_in_image__/out/single_file"));
    }

    if let Some(scratch) = std::env::var_os("BUCK_SCRATCH_PATH") {
        builder.outputs((
            Path::new("/__genrule_in_image__/buck_scratch_path"),
            PathBuf::from(scratch.clone()),
        ));
        builder.setenv((
            "BUCK_SCRATCH_PATH",
            "/__genrule_in_image__/buck_scratch_path",
        ));
    }

    let isol = unshare(builder.build())?;
    let mut cmd = isol.command("bash")?;
    cmd.arg("-e").arg("-c").arg(&args.command);

    let _root_guard = rootless.map(|r| r.escalate()).transpose()?;
    let out = cmd
        .spawn()
        .context(format!("spawn() failed for {:#?}", cmd))?
        .wait()
        .context(format!("wait() failed for {:#?}", cmd))?;
    ensure!(out.success(), "command failed");

    if args.out.dir {
        if let Some((uid, gid)) = rootless.map(|r| r.unprivileged_ids()) {
            for entry in walkdir::WalkDir::new(&args.out.out)
                .into_iter()
                .filter_map(Result::ok)
            {
                std::os::unix::fs::chown(
                    entry.path(),
                    uid.map(|u| u.as_raw()),
                    gid.map(|g| g.as_raw()),
                )?;
            }
        }
    }

    Ok(())
}
