/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::process::Command;

use cap_std::fs::Dir;

pub(crate) struct StubImpl;

impl crate::Stub for StubImpl {
    fn open() -> Dir {
        let archive = File::open("/package.cpio").expect("could not open /package.cpio");
        let out = Command::new("cpio")
            .arg("-idmv")
            .current_dir("/package")
            .stdin(archive)
            .output()
            .expect("failed to run cpio");
        assert!(
            out.status.success(),
            "cpio failed:{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Dir::open_ambient_dir("/package", cap_std::ambient_authority())
            .expect("could not open /package")
    }
}
