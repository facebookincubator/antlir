/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::nspawn;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::anyhow;
use clap::Parser;
use tracing_subscriber::filter;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    subvol: PathBuf,
    #[clap(long)]
    rootless: bool,
    /// `--bind-mount-ro src dst` creates an RO bind-mount of src to dst in the subvol
    #[clap(long, num_args = 2)]
    bind_mount_ro: Vec<PathBuf>,
    /// `--bind-mount-rw src dst` creates an RW bind-mount of src to dst in the subvol
    #[clap(long, num_args = 2)]
    bind_mount_rw: Vec<PathBuf>,
    #[clap(long, overrides_with = "artifacts_require_repo")]
    artifacts_require_repo: bool,
    /// `--user` run command as a given user
    #[clap(long, default_value = "root")]
    user: String,
    #[clap(long, conflicts_with_all = ["boot"])]
    pipe: bool,
    #[clap(long, conflicts_with_all = ["pipe", "rootless"])]
    boot: bool,
    #[clap(long)]
    chdir: Option<PathBuf>,
    #[clap(long)]
    enable_network: bool,
    #[clap(long)]
    /// Don't register the container with systemd-machined (this does not mean that it will always be registered)
    no_register: bool,
    #[clap(last = true)]
    cmd: Vec<OsString>,
}

fn init_logging() {
    let default_filter = filter::Targets::new().with_default(tracing::Level::DEBUG);
    let log_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .with_filter(default_filter);
    tracing_subscriber::registry().with(log_layer).init();
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_logging();

    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while unsharing userns")?;
    }

    let repo_root =
        find_root::find_repo_root(std::env::current_exe().context("while getting argv[0]")?)
            .context("while looking for repo root")?;

    // antlir2_isolate re-parses these into --bind-ro args and escapes any colons, so we
    // instead take an explicit pair to not have to deal with the added complexity of
    // de-and-re-serializing.
    let bind_ro_inputs = args
        .bind_mount_ro
        .chunks(2)
        .map(|pair| match pair {
            [src, dst] => Ok((dst.clone(), src.clone())),
            _ => Err(anyhow!("Unrecognized mount arg: {:?}", pair)),
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;
    let bind_rw = args
        .bind_mount_rw
        .chunks(2)
        .map(|pair| match pair {
            [src, dst] => Ok((dst.clone(), src.clone())),
            _ => Err(anyhow!("Unrecognized mount arg: {:?}", pair)),
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;
    let mut cmd_builder = IsolationContext::builder(args.subvol);
    cmd_builder
        .user(&args.user)
        .inputs(bind_ro_inputs)
        .outputs(bind_rw)
        .ephemeral(true)
        .tmpfs(Path::new("/tmp"))
        .enable_network(args.enable_network);
    if !args.rootless {
        cmd_builder.invocation_type(match (args.boot, args.pipe) {
            (true, false) => InvocationType::BootInteractive,
            (true, true) => unreachable!("--boot and --pipe are mutually exclusive"),
            (false, true) => InvocationType::Pid2Pipe,
            (false, false) => InvocationType::Pid2Interactive,
        });
    } else {
        cmd_builder.devtmpfs(Path::new("/dev"));
    }
    if args.artifacts_require_repo {
        cmd_builder.inputs(repo_root);
        cmd_builder.inputs(PathBuf::from("/usr/local/fbcode"));
    }
    if let Some(chdir) = &args.chdir {
        cmd_builder.working_directory(chdir);
    } else {
        cmd_builder.working_directory(Path::new("/"));
    }

    let mut cmd = args.cmd;
    if args.boot {
        if !args.rootless {
            cmd_builder.register(!args.no_register);
        }
        let container_subtarget_service =
            buck_resources::get("antlir/antlir2/container_subtarget/container-subtarget.service")
                .context("while looking up container-subtarget.service resource")?;
        cmd_builder.inputs((
            PathBuf::from("/run/systemd/system/container-subtarget.service"),
            container_subtarget_service,
        ));
        cmd.push("systemd.unit=container-subtarget.service".into());
    }
    let mut cmd = cmd.into_iter();

    let program = cmd.next().unwrap_or(OsString::from("/bin/bash"));

    let mut command = match args.rootless {
        true => unshare(cmd_builder.build())?.command(program)?,
        false => nspawn(cmd_builder.build())?.command(program)?,
    };

    Err(command.args(cmd).exec().into())
}
