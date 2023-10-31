# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

def _transition_impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch

    constraints = os_transition(
        default_os = attrs.default_os,
        refs = refs,
        constraints = constraints,
        overwrite = True,
    )

    if is_facebook:
        constraints = fb_transition(refs, attrs, constraints)

    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

_transition = transition(
    impl = _transition_impl,
    refs = {
        "arch.aarch64": "ovr_config//cpu/constraints:arm64",
        "arch.x86_64": "ovr_config//cpu/constraints:x86_64",
    } | (
        # @oss-disable
        # @oss-enable {}
    ) | os_transition_refs(),
    attrs = ["default_os", "target_arch"] + (
        # @oss-disable
        [] # @oss-enable
    ),
)

def _configured_alias_impl(ctx: AnalysisContext) -> list[Provider]:
    return ctx.attrs.actual.providers

_configured_alias = rule(
    impl = _configured_alias_impl,
    attrs = {
        "actual": attrs.transition_dep(cfg = _transition),
        "default_os": attrs.string(),
        "target_arch": attrs.option(
            attrs.enum(["x86_64", "aarch64"]),
            default = None,
            doc = "Build for a specific target arch without using `buck -c`",
        ),
    } | (
        # @oss-disable
        # @oss-enable {}
    ),
)

antlir2_configured_alias = rule_with_default_target_platform(_configured_alias)
