/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
