/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "build_mode": env!("BUILD_MODE"),
        }))
        .expect("this is valid json")
    );
}
