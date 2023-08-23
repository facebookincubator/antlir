/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

#[test]
fn built_successfully() {
    assert!(
        Path::new(&std::env::var_os("ANTLIR2_LAYER").expect("ANTLIR2_LAYER env var missing"))
            .exists(),
        "ANTLIR2_LAYER path does not exist"
    );
}
