/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;

fn sig_lines(rpm: &str) -> Vec<String> {
    let out = Command::new("rpm")
        .arg("-v")
        .arg("-K")
        .arg(Path::new("/rpms").join(rpm))
        .output()
        .expect("failed to run cmd");
    let stdout = String::from_utf8(out.stdout).expect("cmd output not utf8");
    assert!(!stdout.is_empty());
    stdout
        .lines()
        .map(|l| l.trim().to_owned())
        .skip(1)
        .collect()
}

#[test]
fn unsigned() {
    assert_eq!(
        sig_lines("unsigned.rpm"),
        vec![
            "Header SHA256 digest: OK",
            "Header SHA1 digest: OK",
            "Payload SHA256 digest: OK",
            "MD5 digest: OK"
        ]
    );
}

#[test]
fn signed() {
    assert_eq!(
        sig_lines("signed.rpm"),
        vec![
            "Header V4 RSA/SHA256 Signature, key ID bf8dba69: OK",
            "Header SHA256 digest: OK",
            "Header SHA1 digest: OK",
            "Payload SHA256 digest: OK",
            "MD5 digest: OK"
        ]
    );
}

#[test]
fn signed_wrong_key() {
    assert_eq!(
        sig_lines("signed-with-wrong-key.rpm"),
        vec![
            "Header V4 RSA/SHA256 Signature, key ID 98f242fa: NOKEY",
            "Header SHA256 digest: OK",
            "Header SHA1 digest: OK",
            "Payload SHA256 digest: OK",
            "MD5 digest: OK"
        ]
    );
}
