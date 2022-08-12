/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;

#[test]
fn used_metalos_bootloader() {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap();
    assert!(
        cmdline.contains("metalos.bootloader=1"),
        "cmdline '{}' did not contain metalos.bootloader=1",
        cmdline
    );
}

// parse (mountpoint, opts) from /proc/mounts
fn parse_mounts() -> BTreeMap<String, String> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap();
    mounts
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields[1].to_string(), fields[3].to_string())
        })
        .collect()
}

fn loaded_kmods() -> Result<HashSet<String>> {
    let mods = std::fs::read_to_string("/proc/modules").context("while reading /proc/modules")?;
    Ok(mods
        .lines()
        .map(|l| l.split_once(' ').unwrap().0.to_string())
        .collect())
}

#[test]
fn kernel_modules_work() -> Result<()> {
    let uname = nix::sys::utsname::uname();
    let mountpoint = format!("/usr/lib/modules/{}", uname.release());
    let mounts = parse_mounts();
    let mount_opts = mounts
        .get(&mountpoint)
        .with_context(|| format!("'{}' not found in /proc/mounts", mountpoint))?;
    assert!(
        mount_opts.contains("subvolid="),
        "kernel mounts should have a subvolid, but it was not present in '{}'",
        mount_opts
    );
    // check to make sure that fuse.ko exists, since it's a very critical
    // module, is not included in the initrd and serves to show that the modules
    // are really present instead of just some arbitrary subvol being mounted
    let fuse_path = Path::new(&mountpoint).join("kernel/fs/fuse/fuse.ko");
    assert!(
        fuse_path.exists(),
        "'{}' does not exist",
        fuse_path.display()
    );
    Command::new("modprobe")
        .arg("fuse")
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    let mods = loaded_kmods().unwrap();
    assert!(mods.contains("fuse"));
    Ok(())
}
