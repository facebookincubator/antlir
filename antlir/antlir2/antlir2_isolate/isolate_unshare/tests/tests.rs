/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::thread;
use std::time::Duration;

use antlir2_isolate::Ephemeral;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use nix::mount::MsFlags;
use nix::mount::mount;
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

/// A program that cannot be executed is named in the error, with the reason.
#[test]
fn explains_permission_denied() {
    let isol = IsolationContext::builder(Path::new("/isolated"))
        .ephemeral(false)
        .working_directory(Path::new("/"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("/not-executable")
        .expect("failed to create command")
        .output()
        .expect("failed to run command");
    assert_cmd_fail(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("while spawning pid2: /not-executable (mode 0444, owner ")
            && stderr.contains("has no execute bit"),
        "{stderr}"
    );
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

/// Loopback interface should be available to bind on
#[test]
fn loopback_interface() {
    // this function is running in an image_test which is already isolated, so
    // no need to spawn another nested isolated process
    assert_eq!(
        std::env::var("container").expect("invalid container env var"),
        "antlir2",
    );
    std::net::TcpListener::bind("[::1]:0").expect("failed to bind to socket");
}

/// Find any ephemeral snapshots for the given layer path, returning their names.
fn find_ephemeral_snapshots(layer: &Path) -> Vec<String> {
    let layer = layer
        .canonicalize()
        .expect("failed to canonicalize layer path");
    let parent = layer.parent().expect("layer has no parent");
    let layer_name = layer.file_name().expect("layer has no name");
    let prefix = format!(".{}.", layer_name.to_string_lossy());
    let mut found = Vec::new();
    for entry in std::fs::read_dir(parent).expect("failed to read parent dir") {
        let entry = entry.expect("failed to read dir entry");
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if name_str.starts_with(&prefix) {
            found.push(name_str);
        }
    }
    found
}

/// Assert that no ephemeral btrfs snapshots were left behind for the given
/// layer path.
fn assert_no_ephemeral_snapshots(layer: &Path) {
    let ephemerals = find_ephemeral_snapshots(layer);
    assert!(
        ephemerals.is_empty(),
        "found ephemeral snapshots that should not be left behind: {:#?}",
        ephemerals
    );
}

/// Ephemeral::Btrfs creates a writable btrfs snapshot and cleans it up after
/// the inner process exits successfully.
#[test]
fn btrfs_ephemeral_cleanup_on_success() {
    let layer = Path::new("/nested/dir/for/symlink/isolated_symlink");
    let isol = IsolationContext::builder(layer)
        .ephemeral(Ephemeral::Btrfs)
        .working_directory(Path::new("/"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("bash")
        .expect("failed to create command")
        .arg("-c")
        .arg("touch /ephemeral_test_file && cat /foo")
        .output()
        .expect("failed to run command");
    assert_cmd_success(&out);
    assert_eq!(out.stdout, b"foo\n");
    assert_no_ephemeral_snapshots(layer);
    assert!(
        !layer.join("ephemeral_test_file").exists(),
        "write inside ephemeral container should not persist to original layer"
    );
}

/// Ephemeral::Btrfs cleans up the snapshot even when the inner process fails.
#[test]
fn btrfs_ephemeral_cleanup_on_failure() {
    let layer = Path::new("/nested/dir/for/symlink/isolated_symlink");
    let isol = IsolationContext::builder(layer)
        .ephemeral(Ephemeral::Btrfs)
        .working_directory(Path::new("/"))
        .build();
    let out = unshare(isol)
        .expect("failed to prepare unshare")
        .command("bash")
        .expect("failed to create command")
        .arg("-c")
        .arg("touch /ephemeral_test_file && exit 42")
        .output()
        .expect("failed to run command");
    assert_cmd_fail(&out);
    assert_eq!(out.status.code().expect("no exit code"), 42);
    assert_no_ephemeral_snapshots(layer);
    assert!(
        !layer.join("ephemeral_test_file").exists(),
        "write inside ephemeral container should not persist to original layer"
    );
}

/// Verify that the ephemeral subvolume is actually visible while a long-running
/// command is executing. This gives us confidence that assert_no_ephemeral_snapshots
/// is looking at the right path and that cleanup assertions are meaningful.
#[test]
fn btrfs_ephemeral_exists_during_execution() {
    let layer = Path::new("/nested/dir/for/symlink/isolated_symlink");
    let isol = IsolationContext::builder(layer)
        .ephemeral(Ephemeral::Btrfs)
        .working_directory(Path::new("/"))
        .build();
    let mut child = unshare(isol)
        .expect("failed to prepare unshare")
        .command("sleep")
        .expect("failed to create command")
        .arg("5")
        .spawn()
        .expect("failed to spawn command");

    // Give the preexec binary time to create the btrfs snapshot
    thread::sleep(Duration::from_secs(2));

    let snapshots = find_ephemeral_snapshots(layer);
    assert!(
        !snapshots.is_empty(),
        "expected to find an ephemeral snapshot while the command is running, but found none"
    );

    let status = child.wait().expect("failed to wait for child");
    assert!(status.success(), "sleep command failed: {}", status);

    // After the command exits, the snapshot should be cleaned up
    assert_no_ephemeral_snapshots(layer);
}
