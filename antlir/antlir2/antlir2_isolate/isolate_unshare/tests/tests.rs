/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;

/// Incredible simple test that basically just makes sure the isolated command
/// runs in a separate root directory.
#[test]
fn simple() {
    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("cat")
        .expect("failed to create command")
        .arg("/foo")
        .output()
        .expect("failed to run command");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"foo\n");
}

/// Confirm that exit codes are propagated up through the standard Command api.
#[test]
fn propagates_exit_code() {
    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("bash")
        .expect("failed to create command")
        .arg("-c")
        .arg("exit 3")
        .output()
        .expect("failed to run command");
    assert!(!out.status.success());
    assert_eq!(out.status.code().expect("no exit code"), 3);
}

/// Check that files can be mounted into the isolated container at an arbitrary
/// path.
#[test]
fn input_binds() {
    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .inputs(("/baz", "/bar"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("cat")
        .expect("failed to create command")
        .arg("/baz")
        .output()
        .expect("failed to run command");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"bar\n");
}
