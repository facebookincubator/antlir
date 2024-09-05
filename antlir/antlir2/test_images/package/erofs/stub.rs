/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::process::Command;

use cap_std::fs::Dir;

pub(crate) fn open() -> Dir {
    let out = Command::new("erofsfuse")
        .arg("/package.erofs")
        .arg("/package")
        .output()
        .expect("failed to run erofsfuse");
    assert!(
        out.status.success(),
        "erofsfuse failed:{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Dir::open_ambient_dir("/package", cap_std::ambient_authority())
        .expect("could not open /package")
}
