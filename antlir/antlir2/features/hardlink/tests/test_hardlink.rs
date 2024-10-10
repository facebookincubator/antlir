/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::MetadataExt;

#[test]
fn inodes_are_the_same() {
    let hello = std::fs::metadata("/hello.txt").expect("failed to stat /hello.txt");
    let aloha = std::fs::metadata("/aloha.txt").expect("failed to stat /aloha.txt");
    assert_eq!(hello.ino(), aloha.ino());
}
