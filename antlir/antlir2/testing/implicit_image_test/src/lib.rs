/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

#[test]
fn built_successfully() {
    let layer_path =
        PathBuf::from(std::env::var_os("ANTLIR2_LAYER").expect("ANTLIR2_LAYER env var missing"));
    assert!(
        layer_path.exists(),
        "ANTLIR2_LAYER path ({}) does not exist",
        layer_path.display()
    );
}
