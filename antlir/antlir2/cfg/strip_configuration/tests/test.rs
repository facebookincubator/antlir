/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[test]
fn test_strip_configuration() {
    assert_eq!(
        std::env::consts::ARCH,
        std::fs::read_to_string("/cpu-arch")
            .expect("failed to read /cpu-arch")
            .trim()
    );
    assert_eq!(
        "no-configuration",
        std::fs::read_to_string("/cpu-arch.unconfigured")
            .expect("failed to read /cpu-arch.unconfigured")
            .trim()
    )
}
