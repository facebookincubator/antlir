/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use anyhow::Result;

fn wait_for_systemd() -> String {
    String::from_utf8(
        Command::new("systemctl")
            .arg("is-system-running")
            .arg("--wait")
            .output()
            .expect("failed to execute 'systemctl is-system-running'")
            .stdout,
    )
    .expect("output not UTF-8")
}

#[test]
fn fetch_unit() -> Result<()> {
    let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let fake_root = Path::new("/unittest_root").join(format!("test_fetch_unit{:?}", ts));
    std::fs::create_dir_all(&fake_root)?;

    assert_eq!("running", wait_for_systemd().trim());

    let mount_output = Command::new("mount")
        .arg("-t")
        .arg("btrfs")
        .arg("/dev/vda")
        .arg(&fake_root)
        .output()
        .expect("failed to start mount command");

    println!("{:#?}", mount_output);
    assert!(mount_output.status.success());

    let ls_output = Command::new("ls")
        .arg(&fake_root)
        .output()
        .expect("failed to start ls command");

    println!("{:#?}", ls_output);

    let fetch_output = Command::new("systemctl")
        .arg("start")
        .arg("metalos-stage.service")
        .output()
        .expect("failed to start metalos-stage.service");

    println!("{:#?}", fetch_output);

    let journal = String::from_utf8(
        Command::new("journalctl")
            .arg("-u")
            .arg("metalos-stage.service")
            .output()
            .expect("failed to get journal output")
            .stdout,
    )
    .expect("output not utf-8");

    println!("journal output: {}", journal);

    assert!(fetch_output.status.success());

    let dir = fake_root.join("volume/image/rootfs/metalos:deadbeefdeadbeefdeadbeefdeadbeef");
    assert!(dir.is_dir(), "{:?} is not a directory: {}", dir, journal);

    let test_file = fake_root
        .join("volume/image/rootfs/metalos:deadbeefdeadbeefdeadbeefdeadbeef/etc/os-release");
    assert!(test_file.exists(), "{:?} does not exist", test_file);

    Ok(())
}
