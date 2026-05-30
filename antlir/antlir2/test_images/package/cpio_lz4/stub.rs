/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;
use std::process::Stdio;

use cap_std::fs::Dir;

pub(crate) struct StubImpl;

impl crate::Stub for StubImpl {
    fn open() -> Dir {
        let mut lz4 = Command::new("lz4")
            .args(["-d", "-c", "/package.cpio.lz4"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to run lz4");
        let out = Command::new("cpio")
            .arg("-idmv")
            .current_dir("/package")
            .stdin(lz4.stdout.take().expect("lz4 stdout was missing"))
            .output()
            .expect("failed to run cpio");
        let lz4_status = lz4.wait().expect("failed waiting for lz4");
        assert!(lz4_status.success(), "lz4 failed: {lz4_status}");
        assert!(
            out.status.success(),
            "cpio failed:{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Dir::open_ambient_dir("/package", cap_std::ambient_authority())
            .expect("could not open /package")
    }
}
