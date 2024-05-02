/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

#[test]
fn this_layer_is_centos9() {
    assert_eq!(
        "centos9",
        std::fs::read_to_string("/os").expect("failed to read /os"),
    );
}

#[test]
fn clones_match_this_layer() {
    assert_eq!(
        "centos9",
        std::fs::read_to_string("/os.cloned.centos8").expect("failed to read /os.cloned.centos8"),
    );
    assert_eq!(
        "centos9",
        std::fs::read_to_string("/os.cloned.centos9").expect("failed to read /os.cloned.centos9"),
    );
}

#[test]
fn packages_use_deps_default_configuration() {
    let out = Command::new("tar")
        .arg("--to-stdout")
        .arg("-xf")
        .arg("/centos9.tar")
        .arg("./os")
        .output()
        .expect("failed to run tar");
    assert!(
        out.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        "centos9",
        std::str::from_utf8(&out.stdout,).expect("tar /os contents are not utf8"),
    );

    let out = Command::new("tar")
        .arg("--to-stdout")
        .arg("-xf")
        .arg("/centos8.tar")
        .arg("./os")
        .output()
        .expect("failed to run tar");
    assert!(
        out.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        "centos8",
        std::str::from_utf8(&out.stdout,).expect("tar /os contents are not utf8"),
    );
}
