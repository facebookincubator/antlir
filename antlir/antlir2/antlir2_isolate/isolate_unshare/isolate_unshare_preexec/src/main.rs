/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use isolate_cfg::Ephemeral;
use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;
use json_arg::Json;
use nix::sched::CloneFlags;
use nix::sched::unshare;
use tracing::warn;

mod isolation;
pub(crate) mod net;
pub(crate) mod new_mount_api;
mod pid1;
use pid1::Pid1Args;

#[derive(Debug, Parser)]
enum Subcommand {
    Main(Main),
    Pid1(Pid1Args),
}

#[derive(Debug, Parser)]
struct Main {
    isolation: Json<IsolationContext<'static>>,
    program: OsString,
    #[clap(last = true)]
    program_args: Vec<OsString>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let args = Subcommand::parse();
    if let Err(e) = match args {
        Subcommand::Main(args) => do_main(args),
        Subcommand::Pid1(args) => pid1::handler(args),
    } {
        let e = format!("{e:#?}");
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn do_main(args: Main) -> Result<()> {
    // Unshare into new pid namespace first, then the rest of the isolation is
    // performed by the first forked process (pid 1) in that namespace
    unshare(CloneFlags::CLONE_NEWPID).context("while unsharing into new pid namespace")?;

    let mut ctx = args.isolation.into_inner();
    let mut snapshot_dir: Option<PathBuf> = None;

    // If Btrfs ephemeral mode is requested, create a writable snapshot of the
    // layer before spawning pid1. The snapshot becomes the new layer with no
    // overlayfs needed (ephemeral is set to None).
    if ctx.ephemeral == Some(Ephemeral::Btrfs) {
        let layer_path = ctx
            .layer
            .canonicalize()
            .with_context(|| format!("while canonicalizing layer path {}", ctx.layer.display()))?;
        let layer_parent = layer_path
            .parent()
            .context("cannot use / as ephemeral source")?;
        let layer_name = layer_path.file_name().context("layer has no file name")?;
        let snap_name = format!(
            ".{}.ephemeral.{}",
            layer_name.to_string_lossy(),
            std::process::id()
        );
        let snap_path = layer_parent.join(&snap_name);

        let subvol = antlir2_btrfs::Subvolume::open(&layer_path).with_context(|| {
            format!(
                "while opening layer as btrfs subvolume: {}",
                layer_path.display()
            )
        })?;
        subvol
            .snapshot(&snap_path, antlir2_btrfs::SnapshotFlags::empty())
            .with_context(|| format!("while creating btrfs snapshot at {}", snap_path.display()))?;

        snapshot_dir = Some(snap_path.clone());
        ctx.layer = Cow::Owned(snap_path);
        ctx.ephemeral = None;
    }

    // For an interactive booted container, the login shell launched by systemd
    // wants to acquire the container's console (/dev/console, which we bind to
    // our inherited controlling terminal) as its controlling terminal. That
    // terminal is owned by the parent user namespace, so the container cannot
    // *steal* it (TIOCSCTTY force requires CAP_SYS_ADMIN in the owning
    // namespace). Release it from our session here — while it is unowned, the
    // container can acquire it cleanly without any capability.
    //
    // SAFETY: these libc calls only manipulate this process's controlling
    // terminal / signal disposition and have no memory safety implications.
    if ctx.invocation_type == InvocationType::BootInteractive && unsafe { libc::isatty(0) } == 1 {
        unsafe {
            // Ignore the SIGHUP that releasing the controlling terminal sends to
            // our (foreground) process group, so it does not kill us.
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            libc::ioctl(0, libc::TIOCNOTTY);
        }
    }

    let mut pid1 = Command::new(std::env::current_exe().context("while getting current exe")?);
    pid1.arg("pid1")
        .arg(serde_json::to_string(&ctx).context("while serializing isolation info")?);
    if ctx.invocation_type.booted() {
        pid1.arg("--exec-init");
    }
    if let Some(ref snap) = snapshot_dir {
        pid1.arg("--snapshot-dir").arg(snap);
    }
    pid1.arg(args.program).arg("--").args(args.program_args);
    let mut pid1 = pid1.spawn().context("while spawning pid1")?;
    let status = pid1.wait().context("while waiting for pid1")?;

    // Fallback cleanup: if pid1 failed to delete the snapshot (e.g. EBUSY or
    // EPERM), try to remove it here.
    if let Some(snap) = &snapshot_dir {
        if snap.exists() {
            // Try btrfs delete first, fall back to a recursive delete
            match antlir2_btrfs::Subvolume::open(snap) {
                Ok(subvol) => {
                    if let Err((_, e)) = subvol.delete() {
                        warn!(
                            "btrfs snapshot delete failed: {e}, falling back to a recursive removal"
                        );
                        if let Err(e) = std::fs::remove_dir_all(snap) {
                            warn!(
                                "fallback removal of snapshot also failed, leaving it in place: {e}"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to open snapshot for cleanup: {e}, falling back to a recursive removal"
                    );
                    if let Err(e) = std::fs::remove_dir_all(snap) {
                        warn!("fallback removal of snapshot also failed, leaving it in place: {e}");
                    }
                }
            }
        }
    }

    if status.success() {
        Ok(())
    } else if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        Err(anyhow!("pid1 failed: {status}"))
    }
}
