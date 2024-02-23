# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
# @oss-disable
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "is_facebook")

def _transition_impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.target_arch:
        target_arch = getattr(refs, "arch." + attrs.target_arch)[ConstraintValueInfo]
        constraints[target_arch.setting.label] = target_arch

    if attrs.default_os:
        constraints = os_transition(
            default_os = attrs.default_os,
            refs = refs,
            constraints = constraints,
            overwrite = True,
        )

    constraints = rootless_cfg.transition(refs = refs, attrs = attrs, constraints = constraints)

    if is_facebook:
        constraints = fb_transition(
            refs = refs,
            attrs = attrs,
            constraints = constraints,
            overwrite = True,
        )

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
        {} # @oss-enable
    ) | os_transition_refs() | rootless_cfg.refs,
    attrs = ["default_os", "target_arch", "rootless"] + (
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
        "default_os": attrs.option(attrs.string(), default = None),
        "labels": attrs.list(attrs.string(), default = []),
        "target_arch": attrs.option(
            attrs.enum(["x86_64", "aarch64"]),
            default = None,
            doc = "Build for a specific target arch without using `buck -c`",
        ),
    } | (
        # @oss-disable
        {} # @oss-enable
    ) | rootless_cfg.attrs,
)

_antlir2_configured_alias_macro = rule_with_default_target_platform(_configured_alias)

def antlir2_configured_alias(
        *,
        name: str,
        default_os: str | None = None,
        **kwargs):
    _antlir2_configured_alias_macro(
        name = name,
        default_os = default_os,
        **kwargs
    )
