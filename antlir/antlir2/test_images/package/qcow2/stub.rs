/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

use cap_std::fs::Dir;

pub(crate) struct StubImpl;

impl crate::Stub for StubImpl {
    fn open() -> Dir {
        let convert = Command::new("qemu-img")
            .arg("convert")
            .arg("-f")
            .arg("qcow2")
            .arg("-O")
            .arg("raw")
            .arg("/package.qcow2")
            .arg("/tmp/package.raw")
            .output()
            .expect("failed to run qemu-img convert");
        assert!(
            convert.status.success(),
            "qemu-img convert failed:{}",
            String::from_utf8_lossy(&convert.stderr)
        );
        let out = Command::new("fuse2fs")
            .arg("/tmp/package.raw")
            .arg("/package")
            .output()
            .expect("failed to run fuse2fs");
        assert!(
            out.status.success(),
            "fuse2fs failed:{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Dir::open_ambient_dir("/package", cap_std::ambient_authority())
            .expect("could not open /package")
    }
}
