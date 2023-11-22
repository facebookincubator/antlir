/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");

use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;
use nix::unistd::Uid;
use tracing::error;
use tracing::trace;
use uuid::Uuid;

mod bind;
use bind::canonicalized_bind;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
    #[error(transparent)]
    Rootless(#[from] antlir2_rootless::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct IsolatedContext {
    program: OsString,
    args: Vec<OsString>,
    env: HashMap<OsString, OsString>,
    #[allow(dead_code)]
    ephemeral_subvol: Option<EphemeralSubvolume>,
}

impl IsolatedContext {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args).arg("--").arg(program);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

/// Isolate the compiler process using `bwrap`.
#[deny(unused_variables)]
pub fn bwrap(ctx: IsolationContext, bwrap: Option<&OsStr>) -> Result<IsolatedContext> {
    let IsolationContext {
        layer,
        working_directory,
        setenv,
        platform,
        inputs,
        outputs,
        invocation_type,
        register,
        user,
        ephemeral,
        tmpfs,
        hostname,
    } = ctx;
    assert_eq!(user, "root", "user != root unimplemented");
    assert!(!register, "register unimplemented");

    let bwrap = bwrap.unwrap_or(OsStr::new("bwrap"));
    let mut bwrap_args = Vec::<OsString>::new();
    let program = match Uid::effective().is_root() {
        true => bwrap.into(),
        false => {
            // TODO(T157360448): don't use sudo when we don't actually need it
            bwrap_args.push(bwrap.into());
            "sudo".into()
        }
    };

    bwrap_args.push("--unshare-cgroup".into());
    bwrap_args.push("--unshare-ipc".into());
    // bwrap always unshares mount
    bwrap_args.push("--unshare-net".into());
    bwrap_args.push("--unshare-pid".into());
    bwrap_args.push("--unshare-uts".into());

    // stop the container if bwrap's parent process (where this code is running)
    // exits
    bwrap_args.push("--die-with-parent".into());
    // detach from this process's controlling terminal
    bwrap_args.push("--new-session".into());
    bwrap_args.push("--hostname".into());
    if let Some(hostname) = hostname {
        bwrap_args.push(hostname.as_ref().into());
    } else {
        bwrap_args.push(Uuid::new_v4().simple().to_string().into());
    }

    // our containers are for isolation, not security, so having all the caps of
    // the parent is desirable when we need to do things like btrfs snapshots
    bwrap_args.push("--cap-add".into());
    bwrap_args.push("ALL".into());

    let rootless = antlir2_rootless::init()?;

    let ephemeral_root = if ephemeral {
        let _guard = rootless.escalate()?;
        let layer = layer.canonicalize()?;
        let mut ephemeral_name = layer.file_name().unwrap_or_default().to_owned();
        ephemeral_name.push(format!(".ephemeral_{}", Uuid::new_v4()));
        let snapshot_path = layer.parent().unwrap_or(&layer).join(&ephemeral_name);
        trace!(
            "snapshotting {} -> {}",
            layer.display(),
            snapshot_path.display()
        );
        let subvol = Subvolume::open(&layer)?;
        let mut snapshot = subvol.snapshot(&snapshot_path, Default::default())?;
        snapshot.set_readonly(false)?;

        bwrap_args.push("--bind".into());
        bwrap_args.push(snapshot_path.into());
        bwrap_args.push("/".into());

        Some(EphemeralSubvolume {
            subvol: Some(snapshot),
            rootless,
        })
    } else {
        bwrap_args.push("--ro-bind".into());
        bwrap_args.push(layer.as_ref().into());
        bwrap_args.push("/".into());
        None
    };

    bwrap_args.push("--dev".into());
    bwrap_args.push("/dev".into());
    bwrap_args.push("--proc".into());
    bwrap_args.push("/proc".into());

    match invocation_type {
        InvocationType::BootReadOnly | InvocationType::Pid2Interactive => {
            todo!("{invocation_type:?}");
        }
        InvocationType::Pid2Pipe => (),
    }

    for path in &tmpfs {
        bwrap_args.push("--tmpfs".into());
        bwrap_args.push(path.as_ref().into());
    }

    if let Some(wd) = &working_directory {
        bwrap_args.push("--chdir".into());
        bwrap_args.push(wd.as_ref().into());
    }
    for (key, val) in &setenv {
        bwrap_args.push("--setenv".into());
        bwrap_args.push(key.into());
        bwrap_args.push(val.into());
    }
    for (dst, src) in &platform {
        let (src, dst) = canonicalized_bind(src, dst)?;
        bwrap_args.push("--ro-bind".into());
        bwrap_args.push(src.into());
        bwrap_args.push(dst.into());
    }
    for (dst, src) in &inputs {
        let (src, dst) = canonicalized_bind(src, dst)?;
        bwrap_args.push("--ro-bind".into());
        bwrap_args.push(src.into());
        bwrap_args.push(dst.into());
    }
    for (dst, out) in &outputs {
        let (out, dst) = canonicalized_bind(out, dst)?;
        bwrap_args.push("--bind".into());
        bwrap_args.push(out.into());
        bwrap_args.push(dst.into());
    }

    trace!("bwrap args: {bwrap_args:?}");

    Ok(IsolatedContext {
        program,
        args: bwrap_args,
        env: Default::default(),
        ephemeral_subvol: ephemeral_root,
    })
}

#[derive(Debug)]
#[must_use]
pub(crate) struct EphemeralSubvolume {
    subvol: Option<Subvolume>,
    rootless: antlir2_rootless::Rootless,
}

impl Drop for EphemeralSubvolume {
    fn drop(&mut self) {
        if let Some(s) = self.subvol.take() {
            let _guard = self.rootless.escalate();
            trace!("deleting subvol {}", s.path().display());
            if let Err((subvol, err)) = s.delete() {
                error!("failed to delete {}: {err}", subvol.path().display());
            }
        }
    }
}
