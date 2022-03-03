/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use derive_builder::Builder;
use goblin::elf::Elf;
use nix::mount::{mount, MsFlags};
use nix::sched::{unshare, CloneFlags};
use once_cell::sync::Lazy;
use regex::Regex;

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
        .collect())
}

#[derive(Debug, Builder, Default)]
#[builder(default, setter(into))]
pub struct SandboxOpts {
    /// Allow readonly access to certain files from the host. The key is the
    /// location on the host and the value is the destination of the bind mount
    /// in the container. The binary and it's .so dependencies are implicitly
    /// included in this list.
    ro_files: HashMap<PathBuf, PathBuf>,
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

    let mut cmd = std::process::Command::new(binary);

    unsafe {
        cmd.pre_exec(move || {
            unshare(
                CloneFlags::CLONE_NEWUSER
                    | CloneFlags::CLONE_NEWNS
                    | CloneFlags::CLONE_NEWNET
                    | CloneFlags::CLONE_NEWCGROUP
                    | CloneFlags::CLONE_NEWUTS,
                // CLONE_NEWPID causes hang for some reason, but it shouldn't
                // really matter
            )?;
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
            // mount the tmpdir as the root of the sandboxed container
            mount::<_, _, str, str>(
                Some(&root_path),
                "/",
                None,
                MsFlags::MS_BIND | MsFlags::MS_RDONLY,
                None,
            )?;
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
    use maplit::hashmap;

    fn is_sandboxed() -> bool {
        std::env::var_os("IN_SANDBOX").is_some()
    }

    fn run_test_in_sandbox(name: &str, opts: SandboxOpts) {
        let path = std::env::current_exe().unwrap();
        let mut cmd = super::sandbox(path, opts).unwrap();
        cmd.arg(name);
        cmd.env("IN_SANDBOX", "1");
        let out = cmd.output().unwrap();
        std::io::stdout().write_all(&out.stdout).unwrap();
        std::io::stderr().write_all(&out.stderr).unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            std::str::from_utf8(&out.stdout).unwrap_or("not utf8"),
            std::str::from_utf8(&out.stderr).unwrap_or("not utf8")
        );
    }

    #[test]
    fn mount_sandboxed() {
        if is_sandboxed() {
            // current exe and its dependencies are visible in this root
            assert!(std::env::current_exe().unwrap().exists());
            // files from the host's root are not visible
            assert!(!Path::new("/etc/os-release").exists());
            // can't just unmount / to gain full access
            assert_eq!(
                nix::errno::Errno::EPERM,
                nix::mount::umount("/").unwrap_err()
            );
        } else {
            run_test_in_sandbox("mount_sandboxed", Default::default());
        }
    }

    #[test]
    fn network_sandboxed() -> Result<()> {
        if is_sandboxed() {
            let ifaddrs: Vec<_> = nix::ifaddrs::getifaddrs()?.collect();
            assert_eq!(
                ifaddrs.len(),
                1,
                "sandbox should only see a single loopback device, instead it sees {:?}",
                ifaddrs
                    .iter()
                    .map(|i| &i.interface_name)
                    .collect::<Vec<_>>()
            );
            assert!(
                ifaddrs[0]
                    .flags
                    .contains(nix::net::if_::InterfaceFlags::IFF_LOOPBACK),
                "single network device in sandbox is not a loopback: {:?}",
                ifaddrs[0]
            );
        } else {
            assert!(nix::ifaddrs::getifaddrs()?.count() > 1);
            run_test_in_sandbox("network_sandboxed", Default::default());
        }
        Ok(())
    }

    #[test]
    fn additional_files() -> Result<()> {
        if is_sandboxed() {
            // now this was explicitly added to the file allowlist
            assert!(!Path::new("/etc/os-release2").exists());
        } else {
            run_test_in_sandbox(
                "additional_files",
                SandboxOpts::builder()
                    .ro_files(hashmap! {"/etc/os-release".into() => "/etc/os-release2".into()})
                    .build()
                    .unwrap(),
            );
        }
        Ok(())
    }
}
