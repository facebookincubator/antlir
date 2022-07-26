/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Test initrd features that boot through the switch-root into the image
// This uses the regular initrd so that it goes through the regular boot
// process, and this unit test is run inside a snapshot of the metalos base
// image.

use anyhow::Context;
use anyhow::Result;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use systemd::Systemd;
use systemd::WaitableSystemState;

async fn wait_for_systemd() -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
    let sd = Systemd::connect(log).await?;
    sd.wait(WaitableSystemState::Starting).await?;
    Ok(())
}

#[tokio::test]
async fn systemd_running() {
    wait_for_systemd().await.unwrap();
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

#[tokio::test]
async fn in_boot_snapshot() {
    wait_for_systemd().await.unwrap();
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .unwrap()
        .trim()
        // systemd's format specifier for boot id strips out dashes
        .replace("-", "");
    let mounts = parse_mounts();
    let rootfs_mount_opts = mounts.get("/").expect("/ not found in /proc/mounts");
    assert!(
        rootfs_mount_opts.contains(&boot_id),
        "could not find boot id '{}' in subvol '{}'",
        boot_id,
        rootfs_mount_opts
    );
}

fn loaded_kmods() -> Result<HashSet<String>> {
    let mods = std::fs::read_to_string("/proc/modules").context("while reading /proc/modules")?;
    Ok(mods
        .lines()
        .map(|l| l.split_once(" ").unwrap().0.to_string())
        .collect())
}

#[tokio::test]
async fn kernel_modules_work() {
    wait_for_systemd().await.unwrap();
    let uname = nix::sys::utsname::uname();
    let mountpoint = format!("/usr/lib/modules/{}", uname.release());
    let mounts = parse_mounts();
    let mount_opts = mounts
        .get(&mountpoint)
        .expect(&format!("'{}' not found in /proc/mounts", mountpoint));
    assert!(
        mount_opts.contains("subvolid="),
        "kernel mount should have a subvolid, but it was not present in '{}'",
        mount_opts
    );
    assert!(
        mount_opts.contains("subvol=/volume/run/kernel/"),
        "kernel mount should point to a snapshot in kernels directory, but mount options disagree '{}'",
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
    let mods = loaded_kmods().unwrap();
    Command::new("modprobe")
        .arg("fuse")
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    let mods = loaded_kmods().unwrap();
    assert!(mods.contains("fuse"));
}
