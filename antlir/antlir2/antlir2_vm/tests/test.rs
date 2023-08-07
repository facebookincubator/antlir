/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[link(name = "c")]
extern "C" {
    fn geteuid() -> u32;
}

#[test]
fn is_root() {
    assert_eq!(0, unsafe { geteuid() });
}
