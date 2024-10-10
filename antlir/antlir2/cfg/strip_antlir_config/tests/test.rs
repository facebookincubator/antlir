/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[test]
fn test_strip_antlir_config() {
    assert_eq!(
        "centos9",
        std::fs::read_to_string("/os-name")
            .expect("failed to read /os-name")
            .trim()
    );
    assert_eq!(
        "no-configuration",
        std::fs::read_to_string("/os-name.unconfigured")
            .expect("failed to read /os-name.unconfigured")
            .trim()
    )
}
