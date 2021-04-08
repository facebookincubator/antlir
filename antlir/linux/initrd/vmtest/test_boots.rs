/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
fn reaches_initrd_target() {
    wait_for_systemd();
    let out = String::from_utf8(
        Command::new("systemd-analyze")
            .arg("time")
            .output()
            .expect("failed to execute 'systemd-analyze time'")
            .stdout,
    )
    .expect("output not UTF-8");
    assert!(out.contains("initrd.target reached"));
}

#[test]
fn not_tainted() {
    wait_for_systemd();
    let out = String::from_utf8(
        Command::new("systemctl")
            .arg("show")
            .arg("--property")
            .arg("Tainted")
            .output()
            .expect("failed to execute 'systemd-analyze time'")
            .stdout,
    )
    .expect("output not UTF-8");
    assert_eq!("Tainted=", out.trim());
}
