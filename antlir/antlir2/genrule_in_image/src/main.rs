/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;
use clap::Parser;
use clap::ValueEnum;
use nix::unistd::Gid;
use nix::unistd::Uid;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    layer: PathBuf,
    #[clap(long)]
    rootless: bool,
    #[clap(value_enum, long)]
    /// On-disk format of the layer storage
    working_format: WorkingFormat,
    /// `--bind-mount-ro src dst` creates an RO bind-mount of src to dst in the subvol
    #[clap(long, num_args = 2)]
    bind_mount_ro: Vec<PathBuf>,
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

    let mut builder = IsolationContext::builder(args.layer.as_path());
    builder.ephemeral(true);
    #[cfg(facebook)]
    builder.platform(["/usr/local/fbcode", "/mnt/gvfs"]);
    let cwd = std::env::current_dir()?;
    builder
        .inputs((
            Path::new("/__genrule_in_image__/working_directory"),
            cwd.as_path(),
        ))
        .inputs((cwd.as_path(), cwd.as_path()))
        .working_directory(Path::new("/__genrule_in_image__/working_directory"))
        .tmpfs(Path::new("/tmp"))
        .devtmpfs(Path::new("/dev"));

    builder.inputs(
        args.bind_mount_ro
            .chunks(2)
            .map(|pair| match pair {
                [src, dst] => Ok((dst.clone(), src.clone())),
                _ => Err(anyhow!("Unrecognized mount arg: {:?}", pair)),
            })
            .collect::<anyhow::Result<HashMap<_, _>>>()?,
    );

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

    let scratch = std::env::var_os("BUCK_SCRATCH_PATH").map(PathBuf::from);
    if let Some(scratch) = scratch.as_ref() {
        builder.outputs((
            Path::new("/__genrule_in_image__/buck_scratch_path"),
            scratch.clone(),
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

    if let Some(scratch) = scratch.as_ref() {
        if let Some((uid, gid)) = rootless.map(|r| r.unprivileged_ids()) {
            chown_r(scratch, uid, gid)?;
        }
    }

    ensure!(out.success(), "command failed");

    if args.out.dir {
        if let Some((uid, gid)) = rootless.map(|r| r.unprivileged_ids()) {
            chown_r(&args.out.out, uid, gid)?;
        }
    }

    Ok(())
}

fn chown_r(dir: &Path, uid: Option<Uid>, gid: Option<Gid>) -> std::io::Result<()> {
    let uid = uid.map(|u| u.as_raw());
    let gid = gid.map(|g| g.as_raw());
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        std::os::unix::fs::chown(entry.path(), uid, gid)?;
    }
    Ok(())
}
