/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use nix::unistd::User;
use nix::unistd::getuid;
use rustix::fs::statfs;

#[test]
fn user() {
    let expected = std::env::var("TEST_USER").expect("TEST_USER not set");
    let actual = whoami::username();
    assert_eq!(expected, actual);
    let expected_uid = User::from_name(&expected)
        .expect("failed to lookup user")
        .expect("no such user")
        .uid;
    assert_eq!(getuid(), expected_uid);
}

#[test]
fn env_propagated() {
    assert_eq!("1", std::env::var("ANTLIR2_TEST").expect("env var missing"));
}

#[test]
fn json_env_quoting() {
    assert_eq!(
        serde_json::json!({
            "foo": "bar"
        }),
        serde_json::from_str::<serde_json::Value>(
            &std::env::var("JSON_ENV").expect("env var missing")
        )
        .expect("invalid json")
    );
}

fn test_tmpfs(path: impl AsRef<Path>) {
    let statfs = statfs(path.as_ref()).expect("failed to statfs");
    assert_eq!(statfs.f_type, 0x01021994, "f_type was not tmpfs"); // TMPFS_MAGIC
}

#[test]
fn tmpfs_tmp() {
    test_tmpfs("/tmp");
}

#[test]
fn tmpfs_run() {
    test_tmpfs("/run");
}

#[test]
fn id_mapping() {
    let meta = std::fs::metadata("/").expect("failed to stat /");
    assert_eq!(0, meta.uid());
    assert_eq!(0, meta.gid());

    let meta = std::fs::metadata("/antlir.txt").expect("failed to stat /antlir.txt");
    assert_eq!(1000, meta.uid());
    assert_eq!(1000, meta.gid());
}
