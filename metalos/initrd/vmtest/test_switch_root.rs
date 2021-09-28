/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Test initrd features that boot through the switch-root into the image
/// This uses the regular initrd so that it goes through the regular boot
/// process, and this unit test is run inside a snapshot of the metalos base
/// image.
use std::process::Command;

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
fn system_running() {
    assert_eq!("running", wait_for_systemd().trim());
}

#[test]
fn in_boot_snapshot() {
    assert_eq!("running", wait_for_systemd().trim());
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .unwrap()
        .trim()
        // systemd's format specifier for boot id strips out dashes
        .replace("-", "");
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap();
    for line in mounts.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields[1] == "/" {
            // don't really care about the exact format, but the current boot id
            // should at least be present in the subvolume mounted at /
            assert!(
                fields[3].contains(&boot_id),
                "could not find boot id '{}' in subvol '{}'",
                boot_id,
                fields[3],
            );
            return;
        }
    }
    panic!("could not find / mount")
}
