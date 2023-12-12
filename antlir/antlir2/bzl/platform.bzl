# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/build_defs.bzl", "config")

def rule_with_default_target_platform(rule_fn):
    def _wrapped(**kwargs):
        for k, v in default_target_platform_kwargs().items():
            kwargs.setdefault(k, v)
        return rule_fn(**kwargs)

    return _wrapped

def default_target_platform_kwargs():
    return {
        "default_target_platform": config.get_platform_for_current_buildfile().target_platform,
    }

def arch_select(aarch64, x86_64) -> Select:
    """Helper for any field that needs arch dependent select"""
    return select({
        "ovr_config//cpu:arm64": aarch64,
        "ovr_config//cpu:x86_64": x86_64,
    })

def arch_to_platform(arch: str) -> str:
    """Helper for converting an arch string to platform name. Mostly useful for
    compatible_with fields."""
    return {
        "aarch64": "ovr_config//cpu:arm64",
        "x86_64": "ovr_config//cpu:x86_64",
    }[arch]
