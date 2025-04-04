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
        let out = Command::new("tar")
            .arg("-xvf")
            .arg("/package.tar")
            .arg("-C")
            .arg("/package")
            .arg("--xattrs")
            .arg("--xattrs-include=user.*")
            .arg("--xattrs-include=security.capability")
            .output()
            .expect("failed to run tar");
        assert!(
            out.status.success(),
            "tar failed:{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Dir::open_ambient_dir("/package", cap_std::ambient_authority())
            .expect("could not open /package")
    }
}
