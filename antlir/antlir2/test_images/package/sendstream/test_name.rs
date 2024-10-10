/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

fn get_first_command(resource: &str) -> Vec<String> {
    let out = Command::new("btrfs")
        .arg("receive")
        .arg("--dump")
        .arg("-f")
        .arg(buck_resources::get(resource).expect("failed to get resource"))
        .output()
        .expect("failed to run btrfs-receive");
    assert!(out.status.success(), "btrfs-receive failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().expect("no lines in stdout");
    first.split_whitespace().map(|p| p.to_owned()).collect()
}

#[test]
fn test_name() {
    let parts = get_first_command("antlir/antlir2/test_images/package/sendstream/NAMED_SENDSTREAM");
    assert_eq!(parts[0], "subvol");
    assert_eq!(parts[1], "./named");
}

#[test]
fn test_name_rootless() {
    let parts = get_first_command(
        "antlir/antlir2/test_images/package/sendstream/NAMED_SENDSTREAM_ROOTLESS",
    );
    assert_eq!(parts[0], "subvol");
    assert_eq!(parts[1], "./named");
}
