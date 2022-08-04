/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_other)]

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use derive_builder::Builder;
use goblin::elf::Elf;
use maplit::btreemap;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::unistd::Gid;
use nix::unistd::Uid;
use once_cell::sync::Lazy;
use regex::Regex;
use seccompiler::BpfProgram;
use seccompiler::SeccompAction;
use seccompiler::SeccompFilter;

/// Simple regex to parse the ouput of `ld.so --list` which is used to resolve
/// the dependencies of a binary.
static LDSO_RE: Lazy<Regex> = Lazy::new(|| {
    regex::RegexBuilder::new(r#"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$"#)
        .multi_line(true)
        .build()
        .unwrap()
});

/// Look up absolute paths to all (recursive) deps of this binary
fn so_dependencies<S: AsRef<OsStr>>(binary: S) -> Result<Vec<PathBuf>> {
    let binary = Path::new(binary.as_ref());
    let buf =
        std::fs::read(&binary).with_context(|| format!("while reading {}", binary.display()))?;
    let elf =
        Elf::parse(&buf).with_context(|| format!("while parsing ELF {}", binary.display()))?;
    let interpreter = elf.interpreter.unwrap_or("/usr/lib64/ld-linux-x86-64.so.2");

    let output = std::process::Command::new(&interpreter)
        .arg("--list")
        .arg(binary)
        .output()
        .with_context(|| format!("failed to list libraries for {:?}", binary))?;
    let ld_output_str = std::str::from_utf8(&output.stdout).context("ld.so output not utf-8")?;

    Ok(LDSO_RE
        .captures_iter(ld_output_str)
        .map(|cap| {
            let path = Path::new(cap.name("path").unwrap().as_str());
            path.into()
        })
        .chain(vec![interpreter.into()])
        .collect())
}

#[derive(Debug, Builder)]
#[builder(default, setter(into))]
pub struct SandboxOpts {
    /// Allow readonly access to certain files from the host. The key is the
    /// location on the host and the value is the destination of the bind mount
    /// in the container. The binary and it's .so dependencies are implicitly
    /// included in this list.
    ro_files: HashMap<PathBuf, PathBuf>,
    /// Blocklist the sandboxed binary from making non-deterministic / unsafe
    /// syscalls.
    seccomp: bool,
}

impl Default for SandboxOpts {
    fn default() -> Self {
        Self {
            seccomp: true,
            ro_files: Default::default(),
        }
    }
}

impl SandboxOpts {
    pub fn builder() -> SandboxOptsBuilder {
        SandboxOptsBuilder::default()
    }
}

/// Sandboxed wrapper for [std::process::Command].
pub struct Command {
    // kept around so that it will be deleted when the Command is dropped
    _root: tempfile::TempDir,
    inner: std::process::Command,
}

impl Deref for Command {
    type Target = std::process::Command;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Command {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Block syscalls that either are:
/// a) likely to lead to non-determinism (such as `gethostname` or `uname`)
/// b) possibly used to communicate outside the sandbox (such as `connect`)
/// c) generally seem dangerous and will fail anyway (such as `reboot`)
fn apply_seccomp_sandbox() -> Result<()> {
    let filter: BpfProgram = SeccompFilter::new(
        btreemap! {
            libc::SYS_accept => vec![],
            libc::SYS_accept4 => vec![],
            libc::SYS_chroot => vec![],
            libc::SYS_clock_adjtime => vec![],
            libc::SYS_clock_nanosleep => vec![],
            libc::SYS_clock_settime => vec![],
            libc::SYS_connect => vec![],
            libc::SYS_getcpu => vec![],
            libc::SYS_geteuid => vec![],
            libc::SYS_getgid => vec![],
            libc::SYS_getpeername => vec![],
            libc::SYS_getsockname => vec![],
            libc::SYS_getsockopt => vec![],
            libc::SYS_getuid => vec![],
            libc::SYS_kexec_file_load => vec![],
            libc::SYS_kexec_load => vec![],
            libc::SYS_keyctl => vec![],
            libc::SYS_mount => vec![],
            libc::SYS_mount_setattr => vec![],
            libc::SYS_nanosleep => vec![],
            libc::SYS_nfsservctl => vec![],
            libc::SYS_personality => vec![],
            libc::SYS_pivot_root => vec![],
            libc::SYS_reboot => vec![],
            libc::SYS_recvfrom => vec![],
            libc::SYS_recvmmsg => vec![],
            libc::SYS_recvmsg => vec![],
            libc::SYS_seccomp => vec![],
            libc::SYS_sendmmsg => vec![],
            libc::SYS_sendmsg => vec![],
            libc::SYS_sendto => vec![],
            libc::SYS_setdomainname => vec![],
            libc::SYS_setgid => vec![],
            libc::SYS_sethostname => vec![],
            libc::SYS_setsockopt => vec![],
            libc::SYS_settimeofday => vec![],
            libc::SYS_setuid => vec![],
            libc::SYS_socket => vec![],
            libc::SYS_socketpair => vec![],
            libc::SYS_swapoff => vec![],
            libc::SYS_swapon => vec![],
            libc::SYS_uname => vec![],
            libc::SYS_unshare => vec![],
        },
        // allow all syscalls not listed above
        SeccompAction::Allow,
        // kill process that makes blocked syscall
        SeccompAction::Trap,
        std::env::consts::ARCH
            .try_into()
            .context("while preparing current arch for seccomp")?,
    )
    .context("while creating SeccompFilter")?
    .try_into()
    .context("while compiling SeccompFilter to bpf program")?;

    seccompiler::apply_filter(&filter).context("while applying seccomp filter")?;
    Ok(())
}

fn write_userns_file(name: &str, contents: &str) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .open(Path::new("/proc/self").join(name))?;
    f.write_all(contents.as_bytes())
}

/// Wrap a binary in a sandbox. This sandbox is not meant for security, so there
/// are no guarantees that it's impossible (or even necessarily difficult) to
/// break out of - but it is designed to be annoying and/or obvious to escape,
/// so as to encourage safe + hermetic code in certain contexts.
pub fn sandbox<S: AsRef<OsStr>>(binary: S, opts: SandboxOpts) -> Result<Command> {
    let root = tempfile::tempdir().context("while creating tmpdir for root")?;
    let root_path = root.path().to_path_buf();

    let mut ro_files = opts.ro_files;
    ro_files.extend(
        so_dependencies(binary.as_ref())?
            .into_iter()
            .map(|p| (p.clone(), p)),
    );
    ro_files.insert(binary.as_ref().into(), binary.as_ref().into());

    let mut cmd = std::process::Command::new(binary);
    // don't pass any env vars that the caller doesn't explicitly ask for
    cmd.env_clear();

    let uid = Uid::current();
    let gid = Gid::current();

    unsafe {
        cmd.pre_exec(move || {
            unshare(
                CloneFlags::CLONE_NEWUSER
                    | CloneFlags::CLONE_NEWCGROUP
                    | CloneFlags::CLONE_NEWNET
                    | CloneFlags::CLONE_NEWNS
                    | CloneFlags::CLONE_NEWUTS,
                // CLONE_NEWPID causes hang for some reason, but it shouldn't
                // really matter
            )?;

            write_userns_file("uid_map", &format!("0 {} 1", uid))?;
            write_userns_file("setgroups", "deny")?;
            write_userns_file("gid_map", &format!("0 {} 1", gid))?;

            for (src, dst) in &ro_files {
                let dst = root_path.join(dst.strip_prefix("/").unwrap());
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::File::create(&dst)?;
                mount::<_, _, str, str>(
                    Some(src),
                    &dst,
                    None,
                    MsFlags::MS_BIND | MsFlags::MS_RDONLY,
                    None,
                )?;
            }
            let proc = root_path.join("proc");
            std::fs::create_dir(&proc)?;
            mount::<_, _, str, str>(
                Some("/proc"),
                &proc,
                None,
                MsFlags::MS_BIND | MsFlags::MS_RDONLY | MsFlags::MS_REC,
                None,
            )?;
            // chroot into the tmpdir as the root of the sandboxed container
            nix::unistd::chroot(&root_path)?;
            if opts.seccomp {
                apply_seccomp_sandbox().map_err(std::io::Error::other)?;
            }
            Ok(())
        });
    }
    Ok(Command {
        _root: root,
        inner: cmd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use maplit::hashmap;
    use serde::Deserialize;
    use serde::Serialize;
    use std::io::Write;
    use strum::IntoEnumIterator;
    use strum_macros::EnumIter;

    fn run_test_in_sandbox<D: Serialize>(name: &str, opts: SandboxOpts, data: D) -> Result<()> {
        let path = std::env::var_os("SANDBOXED_TEST").unwrap();
        let mut cmd = super::sandbox(path, opts).unwrap();
        cmd.arg(name);
        cmd.env("SANDBOX_DATA", serde_json::to_string(&data).unwrap());
        let out = cmd.output().unwrap();
        std::io::stdout().write_all(&out.stdout).unwrap();
        std::io::stderr().write_all(&out.stderr).unwrap();
        match out.status.success() {
            true => Ok(()),
            false => Err(anyhow!(
                "{}\n{}\n{}",
                out.status,
                std::str::from_utf8(&out.stdout).unwrap_or("not utf8"),
                std::str::from_utf8(&out.stderr).unwrap_or("not utf8")
            )),
        }
    }

    #[test]
    fn mount_sandboxed() {
        run_test_in_sandbox("mount_sandboxed", Default::default(), ()).unwrap();
    }

    #[test]
    fn network_sandboxed_seccomp() -> Result<()> {
        run_test_in_sandbox("network_sandboxed_seccomp", Default::default(), ())
            .expect_err("getifaddrs should have been blocked");
        Ok(())
    }

    /// When the seccomp sandbox is disabled, we can verify that network
    /// interfaces normally on the host are missing from within the sandbox
    #[test]
    fn network_sandboxed_no_seccomp() -> Result<()> {
        assert!(nix::ifaddrs::getifaddrs()?.count() > 1);
        run_test_in_sandbox(
            "network_sandboxed_no_seccomp",
            SandboxOpts {
                seccomp: false,
                ..Default::default()
            },
            (),
        )
        .unwrap();
        Ok(())
    }

    #[derive(Debug, Copy, Clone, Serialize, Deserialize, EnumIter)]
    enum Syscall {
        Uname,
        Getuid,
        Geteuid,
        Gethostname,
    }

    #[test]
    fn blocked_syscalls() -> Result<()> {
        for syscall in Syscall::iter() {
            run_test_in_sandbox("blocked_syscalls", Default::default(), syscall)
                .expect_err(&format!("{:?} should have been blocked", syscall));
        }
        Ok(())
    }

    #[test]
    fn additional_files() -> Result<()> {
        run_test_in_sandbox(
            "additional_files",
            SandboxOpts::builder()
                .ro_files(hashmap! {"/etc/os-release".into() => "/etc/os-release2".into()})
                .build()
                .unwrap(),
            (),
        )
        .unwrap();
        Ok(())
    }
}
