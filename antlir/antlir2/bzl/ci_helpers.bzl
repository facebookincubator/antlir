# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode//target_determinator/macros:ci.bzl", "ci")

def _no_aarch64():
    parent = ci.package_get()
    labels = [label for label in parent if "aarch64" not in label]
    ci.package(
        labels,
        overwrite = True,
    )

ci_helpers = struct(
    no_aarch64 = _no_aarch64,
)
