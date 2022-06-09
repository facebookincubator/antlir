/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use find_root::find_repo_root;

fn main() {
    let current = std::env::current_exe().unwrap();
    let current = current
        .as_path()
        .try_into()
        .unwrap_or_else(|_| panic!("{:?} is not absolute", current));
    let repo_root = find_repo_root(current).unwrap();
    println!("{}", repo_root.display());
}
