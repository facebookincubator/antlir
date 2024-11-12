/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use rustix::fs::statfs;

#[test]
fn user() {
    let expected = std::env::var("TEST_USER").expect("TEST_USER not set");
    let actual = whoami::username();
    assert_eq!(expected, actual);
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
