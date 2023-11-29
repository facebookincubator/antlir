/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::process::Command;

#[test]
fn installed_keys() {
    let out = Command::new("rpm")
        .arg("--root")
        .arg("/layer")
        .arg("-q")
        .arg("gpg-pubkey")
        .output()
        .expect("failed to run cmd");
    let stdout = String::from_utf8(out.stdout).expect("cmd output not utf8");
    assert!(!stdout.is_empty());
    let keys: HashSet<_> = stdout.lines().map(|l| l.trim()).collect();
    assert_eq!(
        keys,
        HashSet::from([
            // key that 'signed' is signed with
            "gpg-pubkey-bf8dba69-6524319d",
            // 'unused' key that is also set as trusted for the test repo
            "gpg-pubkey-22b685ee-652452bf",
            // unused key prepended to key.pub but not used to sign any packages
            "gpg-pubkey-efb03108-6524638a",
        ])
    );
}
