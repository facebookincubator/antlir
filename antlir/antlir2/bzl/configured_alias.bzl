# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs")
# @oss-disable[end= ]: load("//antlir/antlir2/bzl/image/facebook:fb_cfg.bzl", "fb_refs", "fb_transition")
load("//antlir/antlir2/cfg/systemd:defs.bzl", "systemd_cfg")
load("//antlir/antlir2/os:cfg.bzl", "os_transition", "os_transition_refs")
load("//antlir/bzl:build_defs.bzl", "get_visibility")
load("//antlir/bzl:internal_external.bzl", "is_facebook")
load("//antlir/bzl:oss_shim.bzl", fb_transition = "ret_none") # @oss-enable

def _transition_impl(platform: PlatformInfo, refs: struct, attrs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    if attrs.default_os:
        constraints = os_transition(
            default_os = attrs.default_os,
            refs = refs,
            constraints = constraints,
            overwrite = True,
        )

    constraints = rootless_cfg.transition(
        refs = refs,
        attrs = attrs,
        constraints = constraints,
        overwrite = True,
    )
    constraints = systemd_cfg.transition(
        constraints = constraints,
        refs = refs,
        attrs = attrs,
        overwrite = True,
    )

    if is_facebook:
        constraints = fb_transition(
            refs = refs,
            attrs = attrs,
            constraints = constraints,
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
        # @oss-disable[end= ]: fb_refs
        {} # @oss-enable
    ) | os_transition_refs() | rootless_cfg.refs | systemd_cfg.refs,
    attrs = cfg_attrs().keys(),
)

def _configured_alias_impl(ctx: AnalysisContext) -> list[Provider]:
    return ctx.attrs.actual.providers

_configured_alias = rule(
    impl = _configured_alias_impl,
    attrs = {
        "actual": attrs.transition_dep(cfg = _transition),
        "labels": attrs.list(attrs.string(), default = []),
    } | cfg_attrs(),
)

_antlir2_configured_alias_macro = rule_with_default_target_platform(_configured_alias)

def antlir2_configured_alias(
        *,
        name: str,
        default_os: str | Select | None = None,
        rootless: bool | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    if rootless == None:
        rootless = get_antlir2_rootless()
    if not rootless:
        kwargs["labels"] = selects.apply(kwargs.pop("labels", []), lambda labels: list(labels) + ["uses_sudo"])
    _antlir2_configured_alias_macro(
        name = name,
        default_os = default_os,
        rootless = rootless,
        visibility = get_visibility(visibility),
        **kwargs
    )
