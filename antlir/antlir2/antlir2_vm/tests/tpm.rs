/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

#[test]
fn tpm_exists() {
    assert!(Path::new("/dev/tpm0").exists());
    assert!(Path::new("/dev/tpmrm0").exists());
}
