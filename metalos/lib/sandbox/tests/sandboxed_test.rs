/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use nix::net::if_::InterfaceFlags;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

#[derive(Parser)]
struct Args {
    test: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.test.as_str() {
        "mount_sandboxed" => Ok(mount_sandboxed()),
        "network_sandboxed_seccomp" => network_sandboxed_seccomp(),
        "network_sandboxed_no_seccomp" => network_sandboxed_no_seccomp(),
        "blocked_syscall" => blocked_syscalls(),
        "additional_files" => additional_files(),
        _ => panic!("unknown test {}", args.test),
    }
}

fn sandbox_data<D: DeserializeOwned>() -> D {
    serde_json::from_str(&std::env::var("SANDBOX_DATA").unwrap()).unwrap()
}

fn mount_sandboxed() {
    // current exe and its dependencies are visible in this root
    assert!(std::env::current_exe().unwrap().exists());
    // files from the host's root are not visible
    assert!(!Path::new("/etc/os-release").exists());
    // can't just unmount / to gain full access
    assert_eq!(
        // EINVAL since mountpoint is locked (it comes from a higher-privileged namespace)
        nix::errno::Errno::EINVAL,
        nix::mount::umount("/").unwrap_err()
    );
}

fn network_sandboxed_seccomp() -> Result<()> {
    nix::ifaddrs::getifaddrs().unwrap();
    Ok(())
}

/// When the seccomp sandbox is disabled, we can verify that network
/// interfaces normally on the host are missing from within the sandbox
fn network_sandboxed_no_seccomp() -> Result<()> {
    let mut ifaddrs: HashMap<String, InterfaceFlags> = nix::ifaddrs::getifaddrs()?
        .map(|i| (i.interface_name, i.flags))
        .collect();

    assert!(
        ifaddrs.contains_key("lo"),
        "missing loopback interface: {:?}",
        ifaddrs
    );
    let lo = ifaddrs.remove("lo").unwrap();
    assert!(
        lo.contains(InterfaceFlags::IFF_LOOPBACK),
        "lo was not a loopback, what? {:?}",
        lo
    );

    // there might be two other interfaces for ipv6 tunneling that are
    // created by kernel modules so are still present in the namespace
    if !ifaddrs.is_empty() {
        ifaddrs.remove("ip6tnl0");
        ifaddrs.remove("tunl0");
        assert!(
            ifaddrs.is_empty(),
            "unexpected interfaces available in sandbox: {:?}",
            ifaddrs,
        )
    }
    Ok(())
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
enum Syscall {
    Uname,
    Getuid,
    Geteuid,
    Gethostname,
}

fn blocked_syscalls() -> Result<()> {
    let syscall: Syscall = sandbox_data();
    match syscall {
        Syscall::Uname => {
            nix::sys::utsname::uname();
        }
        Syscall::Getuid => {
            nix::unistd::getuid();
        }
        Syscall::Geteuid => {
            nix::unistd::getuid();
        }
        Syscall::Gethostname => {
            let mut buf = [0u8; 64];
            nix::unistd::gethostname(&mut buf).unwrap();
        }
    }
    Ok(())
}

fn additional_files() -> Result<()> {
    // now this was explicitly added to the file allowlist
    assert!(Path::new("/etc/os-release2").exists());
    Ok(())
}
