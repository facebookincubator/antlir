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
use std::path::PathBuf;
use std::process::Command;

use crate::IsolatedCompilerContext;
use crate::IsolationContext;

fn try_canonicalize<P: AsRef<Path>>(path: P) -> PathBuf {
    std::fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_owned())
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

/// Isolate the compiler process using `systemd-nspawn`.
pub fn nspawn_compiler(ctx: &IsolationContext) -> IsolatedCompilerContext {
    let mut cmd = Command::new("sudo");
    cmd.arg("systemd-nspawn")
        .arg("--quiet")
        .arg("--directory")
        .arg(ctx.build_appliance)
        .arg("--ephemeral")
        // TODO(vmagro): we might actually want to implement real pid1 semantics
        // in the compiler process for better control, but for now let's not
        .arg("--as-pid2")
        .arg("--register=no")
        .arg("--private-network");
    if let Some(wd) = &ctx.working_directory {
        cmd.arg("--chdir").arg(wd);
    }
    for (key, val) in &ctx.setenv {
        let mut arg = OsString::from(key);
        arg.push("=");
        arg.push(val);
        cmd.arg("--setenv").arg(arg);
    }
    for platform in &ctx.compiler_platform {
        cmd.arg("--bind-ro")
            .arg(escape_bind(try_canonicalize(platform).as_os_str()));
    }
    for src in &ctx.image_sources {
        cmd.arg("--bind-ro")
            .arg(escape_bind(try_canonicalize(src).as_os_str()));
    }
    for out in &ctx.writable_outputs {
        cmd.arg("--bind")
            .arg(escape_bind(try_canonicalize(out).as_os_str()));
    }
    let mut out_arg = escape_bind(try_canonicalize(ctx.root).as_os_str()).to_os_string();
    out_arg.push(":/out");
    cmd.arg("--bind").arg(out_arg);

    // caller will add the compiler path as the first argument
    cmd.arg("--");

    IsolatedCompilerContext {
        root: "/out".into(),
        command: cmd,
    }
}
