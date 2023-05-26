/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::Command;

use nix::unistd::Uid;
use uuid::Uuid;

use crate::IsolatedContext;
use crate::IsolationContext;

fn try_canonicalize<'a>(path: &'a Path) -> Cow<'a, Path> {
    std::fs::canonicalize(path).map_or(Cow::Borrowed(path), Cow::Owned)
}

/// 'systemd-nspawn' accepts ':' as a special delimiter in args to '--bind[-ro]'
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
        arg.push(escape_bind(try_canonicalize(dst).as_os_str()));
        Cow::Owned(arg)
    }
}

/// Isolate the compiler process using `systemd-nspawn`.
#[deny(unused_variables)]
pub fn nspawn(ctx: IsolationContext) -> IsolatedContext {
    let IsolationContext {
        layer,
        working_directory,
        setenv,
        platform,
        inputs,
        outputs,
        boot,
        register,
        user,
        ephemeral,
    } = ctx;
    let mut cmd = match Uid::effective().is_root() {
        true => Command::new("systemd-nspawn"),
        false => {
            let mut cmd = Command::new("sudo");
            cmd.arg("systemd-nspawn");
            cmd
        }
    };
    cmd.arg("--quiet")
        .arg("--pipe")
        .arg("--directory")
        .arg(layer.as_ref())
        .arg("--private-network")
        .arg("--user")
        .arg(user.as_ref())
        // keep whatever timezone was in the image, not on the host
        .arg("--timezone=off")
        // Don't pollute the host's /var/log/journal
        .arg("--link-journal=no")
        // Explicitly do not look for any settings for our ephemeral machine
        // on the host.
        .arg("--settings=no");
    if ephemeral {
        cmd.arg("--ephemeral");
    }
    if !boot {
        // TODO(vmagro): we might actually want to implement real pid1 semantics
        // in the compiler process for better control, but for now let's not
        cmd.arg("--as-pid2");
    } else {
        cmd.arg("--boot");
    }
    if register {
        cmd.arg(format!("--machine={}", Uuid::new_v4()));
    } else {
        cmd.arg("--register=no");
        cmd.arg("--keep-unit");
    }
    if let Some(wd) = &working_directory {
        cmd.arg("--chdir").arg(wd.as_ref());
    }
    for (key, val) in &setenv {
        let mut arg = OsString::from(key);
        arg.push("=");
        arg.push(val);
        cmd.arg("--setenv").arg(arg);
    }
    for (dst, src) in &platform {
        cmd.arg("--bind-ro").arg(bind_arg(dst, src));
    }
    for (dst, src) in &inputs {
        cmd.arg("--bind-ro").arg(bind_arg(dst, src));
    }
    for (dst, out) in &outputs {
        cmd.arg("--bind").arg(bind_arg(dst, out));
    }

    // caller will add the compiler path as the first argument
    cmd.arg("--");

    IsolatedContext { command: cmd }
}
