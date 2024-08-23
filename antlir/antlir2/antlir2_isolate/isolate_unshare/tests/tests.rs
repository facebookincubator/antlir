/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::path::Path;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use nix::mount::mount;
use nix::mount::MsFlags;
use tempfile::TempDir;

fn assert_cmd_success(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "failed {}: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn assert_cmd_fail(out: &std::process::Output) {
    assert!(
        !out.status.success(),
        "command did not fail: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

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
    assert_cmd_fail(&out);
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
    assert_cmd_success(&out);
    assert_eq!(out.stdout, b"bar\n");
}

/// When mounting an input directory, it must be readonly.
#[test]
fn inputs_are_readonly() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .inputs(("/input", dir.path()))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("bash")
        .expect("failed to create command")
        .arg("-c")
        .arg("touch /input/bar")
        .output()
        .expect("failed to run command");
    assert_cmd_fail(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        "touch: cannot touch '/input/bar': Read-only file system",
        stderr.trim()
    );
}

/// When mounting an input directory with recursive bind mounts, they all should
/// be readonly.
#[test]
fn recursive_inputs_are_readonly() {
    let bottom = TempDir::new().expect("failed to create temp dir");
    let top = TempDir::new().expect("failed to create temp dir");
    std::fs::create_dir(top.path().join("bottom")).expect("failed to create mountpoint");
    mount(
        Some(bottom.path()),
        top.path().join("bottom").as_path(),
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .expect("failed to do bind mount");

    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .inputs(("/input", top.path()))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("bash")
        .expect("failed to create command")
        .arg("-c")
        .arg("/usr/bin/touch /input/bottom/bar")
        .output()
        .expect("failed to run command");
    assert_cmd_fail(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        "/usr/bin/touch: cannot touch '/input/bottom/bar': Read-only file system",
        stderr.trim()
    );
}

/// When mounting the eden repo as an input, it should be readonly.
#[cfg(facebook)]
#[test]
fn repo_mount_is_readonly() {
    assert!(
        Path::new(".eden").exists(),
        "this test must be run with an eden repo"
    );
    let err = std::fs::write("foo", "bar\n").expect_err("should fail to write into repo");
    assert!(err.kind() == std::io::ErrorKind::ReadOnlyFilesystem);
    let err = std::fs::write("buck-out/foo", "bar\n").expect_err("should fail to write into repo");
    assert!(
        err.kind() == std::io::ErrorKind::ReadOnlyFilesystem
            || err.kind() == std::io::ErrorKind::PermissionDenied,
        "expected EROFS or EPERM, but got {}",
        err.kind()
    );
}
