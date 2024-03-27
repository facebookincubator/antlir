/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use find_root::find_repo_root;

fn main() {
    let current = std::env::current_exe().expect("while getting argv[0]");
    let repo_root = find_repo_root(current)
        .expect("not in repo")
        .canonicalize()
        .expect("failed to canonicalize repo root dir");
    println!("{}", repo_root.display());
}
