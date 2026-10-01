/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// The cpio archive must record the image root directory (`.`) with its mode:
// the kernel applies it to the live rootfs at initramfs unpack time
// (init_chmod in init/initramfs.c). GNU cpio itself ignores the `.` mode when
// extracting onto a pre-existing directory, so assert it here at the archive
// level instead of in the VM test, which only sees the extracted tree.

use std::fs::File;
use std::io::BufReader;

#[test]
fn root_mode() {
    let path = buck_resources::get("antlir/antlir2/test_images/package/cpio/test.cpio")
        .expect("failed to get test.cpio resource");
    let input = BufReader::new(File::open(path).expect("failed to open test.cpio"));

    let reader = cpio::NewcReader::new(input).expect("failed to read first cpio entry");
    let (name, mode) = (reader.entry().name().to_owned(), reader.entry().mode());
    // The packager emits entries LANG=C sorted, which puts `.` first.
    assert_eq!(name, ".", "first cpio entry is not the root dir");
    assert_eq!(
        mode, 0o040755,
        "first entry mode is not 040755, got {mode:o}",
    );
}
