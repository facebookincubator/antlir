/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;

#[test]
fn test_env() {
    assert_eq!(env::var("kitteh").unwrap(), "meow");
    assert_eq!(env::var("dogsgo").unwrap(), "woof");
}
