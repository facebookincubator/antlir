# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/bzl:wrap_runtime_deps.bzl",
    helper = "maybe_wrap_executable_target",
)
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_wrap_executable_target_rule",
)

def maybe_wrap_executable_target(target, wrap_suffix, **kwargs):
    kwargs.update({"wrap_rule_fn": maybe_wrap_executable_target_rule})
    return helper(target, wrap_suffix, **kwargs)
