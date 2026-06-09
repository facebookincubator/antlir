/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use clap::Parser;
use json_arg::JsonFile;
use nix::unistd::Gid;
use nix::unistd::Uid;
use tracing::debug;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    root: PathBuf,
    #[clap(long)]
    pass_env: Vec<String>,
    #[clap(long)]
    env: Option<JsonFile<BTreeMap<String, Vec<String>>>>,
    #[clap(required(true), trailing_var_arg(true), allow_hyphen_values(true))]
    command: Vec<String>,
    #[clap(long)]
    /// If set, don't unshare into a fully remapped antlir userns, just unshare
    /// and map the current (ug)id to root and hope that it's good enough for
    /// what we're going to do (it usually is)
    single_user_userns: bool,
    #[clap(long)]
    /// If set, mount the root layer read-only instead of setting up an
    /// ephemeral overlayfs on top of it. This avoids the need for overlayfs,
    /// which is useful when the command does not need to write into the root
    /// (it only writes to bind-mounted outputs and /tmp).
    readonly_root: bool,
}

fn main() -> Result<()> {
    let mut args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::TRACE)
        .init();

    if !args.single_user_userns {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    } else {
        let uid = Uid::current();
        let gid = Gid::current();
        nix::sched::unshare(nix::sched::CloneFlags::CLONE_NEWUSER)
            .context("while unsharing into new userns")?;
        std::fs::write("/proc/self/uid_map", format!("0 {uid} 1\n"))
            .context("while writing uid_map")?;
        nix::unistd::setuid(Uid::from_raw(0)).context("while setting uid to 0")?;
        std::fs::write("/proc/self/setgroups", "deny\n").context("while writing setgroups")?;
        std::fs::write("/proc/self/gid_map", format!("0 {gid} 1\n"))
            .context("while writing gid_map")?;
        nix::unistd::setgid(Gid::from_raw(0)).context("while setting gid to 0")?;
    }

    let mut builder = IsolationContext::builder(&args.root);
    if args.readonly_root {
        builder.readonly().ephemeral(false);
    } else {
        builder.ephemeral(true);
    }
    #[cfg(facebook)]
    builder.platform(["/usr/local/fbcode", "/mnt/gvfs"]);
    let cwd = std::env::current_dir()?;

    // We need to bind mount buck-out (which lives under our cwd) into the
    // target layer. Find the *deepest* cwd path prefix that already exists in
    // the layer and bind mount our cwd-subtree there.
    //
    // It is important to bind over a directory that already exists rather than
    // the shortest prefix that *doesn't* exist (which would require creating a
    // fresh mountpoint dir). When the root is a plain bind mount of the layer
    // (i.e. not an ephemeral overlay), creating that mountpoint writes into the
    // shared, on-disk layer artifact and persists. Successive actions would
    // then walk progressively deeper (e.g. /data -> /data/users -> ...) until
    // the entire cwd exists in the layer, at which point no bind mount is added
    // at all and the command can no longer see buck-out. Binding over an
    // existing directory needs no mkdir, so the layer is never mutated and the
    // selection is deterministic regardless of how many actions have run.
    let cwd_vec = cwd.components().collect::<Vec<_>>();
    if cwd_vec.len() > 1 {
        let layer_root = fs::canonicalize(&args.root)?;
        let mut chosen: Option<PathBuf> = None;
        for i in 1..=(cwd_vec.len() - 1) {
            let cwd_prefix = cwd_vec[1..=i].iter().collect::<PathBuf>();
            if layer_root.join(&cwd_prefix).exists() {
                chosen = Some(cwd_prefix);
            } else {
                // The deepest existing prefix is the previous iteration's. If
                // nothing has existed yet (the layer lacks even the top-level
                // dir), fall back to binding this shortest prefix and let the
                // mountpoint be created.
                chosen = chosen.or(Some(cwd_prefix));
                break;
            }
        }
        if let Some(cwd_prefix) = chosen {
            let path = Path::new(&Component::RootDir).join(cwd_prefix);
            debug!("bind-mounting {path:?} as a way to get cwd");
            builder.outputs(path);
        }
    }

    if let Some(env) = args.env.map(JsonFile::into_inner) {
        for (k, mut v) in env {
            ensure!(
                v.len() == 1,
                "env var '{k}' expanded to multiple values: {v:#?}"
            );
            builder.setenv((k, v.remove(0)));
        }
    }
    for e in args.pass_env {
        if let Some(v) = std::env::var_os(&e) {
            builder.setenv((e, v));
        }
    }

    builder
        .working_directory(cwd.as_path())
        .tmpfs(Path::new("/tmp"))
        .devtmpfs(Path::new("/dev"));

    let isol = unshare(builder.build())?;
    let mut cmd = isol.command(args.command.remove(0))?;
    cmd.args(args.command);
    sleep(Duration::from_secs(0));
    let out = cmd
        .spawn()
        .context(format!("spawn() failed for {:#?}", cmd))?
        .wait()
        .context(format!("wait() failed for {:#?}", cmd))?;
    ensure!(out.success(), "command failed");

    Ok(())
}
