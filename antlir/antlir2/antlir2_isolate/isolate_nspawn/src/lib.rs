/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::Command;

use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;
use nix::unistd::Uid;
use uuid::Uuid;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported setting '{0}'")]
    Unsupported(&'static str),
}

#[derive(Debug)]
pub struct IsolatedContext {
    program: OsString,
    args: Vec<OsString>,
    env: HashMap<OsString, OsString>,
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

fn try_canonicalize<'a>(path: &'a Path) -> Cow<'a, Path> {
    std::fs::canonicalize(path).map_or(Cow::Borrowed(path), Cow::Owned)
}

/// 'systemd-nspawn' accepts ':' as a special delimiter in nspawn_args to '--bind[-ro]'
/// in the form of 'SRC[:DST[:OPTS]]'. If a path we're trying to mount into the
/// container ends up with a ':' in it, we need to escape it ahead of time.
fn escape_bind<'a>(s: &'a OsStr) -> Cow<'a, OsStr> {
    if s.as_bytes().contains(&b':') {
        let mut v = s.as_bytes().to_vec();
        let colons: Vec<_> = v
            .iter()
            .enumerate()
            .filter_map(|(idx, byte)| if *byte == b':' { Some(idx) } else { None })
            .rev()
            .collect();
        for idx in colons {
            v.splice(idx..idx + 1, b"\\:".to_vec());
        }
        Cow::Owned(OsString::from_vec(v))
    } else {
        Cow::Borrowed(s)
    }
}

fn bind_arg<'a>(dst: &'a Path, src: &'a Path) -> Cow<'a, OsStr> {
    if dst == src {
        Cow::Owned(escape_bind(try_canonicalize(dst).as_os_str()).into_owned())
    } else {
        let mut arg = escape_bind(try_canonicalize(src).as_os_str()).into_owned();
        arg.push(":");
        arg.push(escape_bind(dst.as_os_str()));
        Cow::Owned(arg)
    }
}

/// Isolate the compiler process using `systemd-nspawn`.
#[deny(unused_variables)]
pub fn nspawn(ctx: IsolationContext) -> Result<IsolatedContext> {
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
        devtmpfs,
        hostname,
        readonly,
    } = ctx;
    if !devtmpfs.is_empty() && devtmpfs.len() > 1 && !devtmpfs.contains(Path::new("/dev")) {
        return Err(Error::Unsupported("devtmpfs"));
    }

    let mut nspawn_args = Vec::<OsString>::new();
    let mut env = HashMap::new();
    let program = match Uid::effective().is_root() {
        true => "systemd-nspawn",
        false => {
            nspawn_args.push("systemd-nspawn".into());
            "sudo"
        }
    };
    nspawn_args.push("--quiet".into());
    nspawn_args.push("--directory".into());
    nspawn_args.push(layer.as_ref().into());
    nspawn_args.push("--private-network".into());
    nspawn_args.push("--user".into());
    nspawn_args.push(user.as_ref().into());
    if let Some(hostname) = hostname {
        nspawn_args.push("--hostname".into());
        nspawn_args.push(hostname.as_ref().into());
    }
    // keep whatever timezone was in the image, not on the host
    nspawn_args.push("--timezone=off".into());
    // Don't pollute the host's /var/log/journal
    nspawn_args.push("--link-journal=no".into());
    // Explicitly do not look for any settings for our ephemeral machine
    // on the host.
    nspawn_args.push("--settings=no".into());
    if ephemeral {
        nspawn_args.push("--ephemeral".into());
        // run as many ephemeral containers as we want
        env.insert("SYSTEMD_NSPAWN_LOCK".into(), "0".into());
    }
    if readonly {
        nspawn_args.push("--read-only".into());
        env.insert("SYSTEMD_NSPAWN_LOCK".into(), "0".into());
    }
    match invocation_type {
        InvocationType::Pid2Interactive => {
            nspawn_args.push("--console=interactive".into());
            // TODO(vmagro): we might actually want to implement real pid1 semantics
            // in the compiler process for better control, but for now let's not
            nspawn_args.push("--as-pid2".into());
        }
        InvocationType::Pid2Pipe => {
            nspawn_args.push("--console=pipe".into());
            // TODO(vmagro): we might actually want to implement real pid1 semantics
            // in the compiler process for better control, but for now let's not
            nspawn_args.push("--as-pid2".into());
        }
        InvocationType::BootReadOnly => {
            nspawn_args.push("--boot".into());
            nspawn_args.push("--console=read-only".into());
        }
    }
    if register {
        nspawn_args.push(format!("--machine={}", Uuid::new_v4()).into());
    } else {
        nspawn_args.push("--register=no".into());
        if !invocation_type.booted() {
            // In a booted container, let systemd-nspawn create a transient
            // scope unit so that cgroup management by the booted systemd works
            // as expected, regardless of any questionable environment
            // surrounding this antlir2_isolate call. This doesn't matter for
            // non-booted containers since they shouldn't be doing anything with
            // cgroups (other than whatever systemd-nspawn is doing)
            nspawn_args.push("--keep-unit".into());
        }
    }

    for path in &tmpfs {
        nspawn_args.push("--tmpfs".into());
        nspawn_args.push(path.as_ref().into());
    }

    if let Some(wd) = &working_directory {
        nspawn_args.push("--chdir".into());
        nspawn_args.push(wd.as_ref().into());
    }
    for (key, val) in &setenv {
        let mut arg = OsString::from(key);
        arg.push("=");
        arg.push(val);
        nspawn_args.push("--setenv".into());
        nspawn_args.push(arg);
    }
    for (dst, src) in &platform {
        nspawn_args.push("--bind-ro".into());
        nspawn_args.push(bind_arg(dst, src).into());
    }
    for (dst, src) in &inputs {
        nspawn_args.push("--bind-ro".into());
        nspawn_args.push(bind_arg(dst, src).into());
    }
    for (dst, out) in &outputs {
        nspawn_args.push("--bind".into());
        nspawn_args.push(bind_arg(dst, out).into());
    }
    nspawn_args.push("--capability=all".into());
    env.insert("SYSTEMD_SECCOMP".into(), "0".into());

    Ok(IsolatedContext {
        program: program.into(),
        args: nspawn_args,
        env,
    })
}
