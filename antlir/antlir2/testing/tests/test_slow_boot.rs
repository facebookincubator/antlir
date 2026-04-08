/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Test that boots with a dependency on slow-discovery.service.
//! Without static listing, test discovery would need to boot the container
//! (and wait for slow-discovery.service) just to enumerate tests.

use std::process::Command;

#[test]
fn slow_unit_completed() {
    let output = Command::new("systemctl")
        .args(["show", "-p", "Result", "slow-discovery.service"])
        .output()
        .expect("failed to run systemctl");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "Result=success",
        "slow-discovery.service should have succeeded"
    );
}
