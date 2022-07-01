/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;
use std::fs;
use std::io::Read;
use std::io::Write;

#[test]
fn env() {
    assert_eq!(env::var("kitteh").unwrap(), "meow");
    assert_eq!(env::var("dogsgo").unwrap(), "woof");
}

#[link(name = "c")]
extern "C" {
    fn geteuid() -> u32;
}

#[test]
fn root_user() {
    unsafe {
        assert_eq!(geteuid(), 0);
    }
}

#[test]
fn rootfs_writable() {
    {
        let mut fw = fs::File::create("/some_path").unwrap();
        fw.write_all(b"content").unwrap();
    }
    let mut fr = fs::File::open("/some_path").unwrap();
    let mut contents = String::new();
    fr.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "content");
}
