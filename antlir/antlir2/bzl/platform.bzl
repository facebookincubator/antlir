# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/build_defs.bzl", "config")

def rule_with_default_target_platform(rule_fn):
    def _wrapped(**kwargs):
        kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform
        return rule_fn(**kwargs)

    return _wrapped
