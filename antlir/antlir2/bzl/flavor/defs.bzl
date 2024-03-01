# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "FlavorDnfInfo", "FlavorInfo", "LayerInfo")
# @oss-disable
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "RepoSetInfo")

_flavor_attrs = {
    "default_build_appliance": attrs.dep(providers = [LayerInfo]),
    "default_dnf_excluded_rpms": attrs.list(
        attrs.string(),
        default = [],
    ),
    "default_dnf_extra_repo_set": attrs.option(
        attrs.dep(providers = [RepoSetInfo]),
        default = None,
    ),
    "default_dnf_repo_set": attrs.dep(providers = [RepoSetInfo]),
    "default_dnf_versionlock": attrs.option(
        attrs.source(),
        default = None,
    ),
    "rpm_reflink_flavor": attrs.option(attrs.string(), default = None),
}

def _impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        FlavorInfo(
            default_build_appliance = ctx.attrs.default_build_appliance,
            dnf_info = FlavorDnfInfo(
                default_excluded_rpms = ctx.attrs.default_dnf_excluded_rpms,
                default_extra_repo_set = ctx.attrs.default_dnf_extra_repo_set,
                default_repo_set = ctx.attrs.default_dnf_repo_set,
                default_versionlock = ctx.attrs.default_dnf_versionlock,
                reflink_flavor = ctx.attrs.rpm_reflink_flavor,
            ),
            label = ctx.label,
        ),
        DefaultInfo(sub_targets = {
            "default_build_appliance": ctx.attrs.default_build_appliance.providers,
            "default_versionlock": [DefaultInfo(ctx.attrs.default_dnf_versionlock)],
        }),
    # @oss-disable
    ] # @oss-enable

_flavor = rule(
    impl = _impl,
    attrs = _flavor_attrs,
)

flavor = rule_with_default_target_platform(_flavor)

def _overridden_attr(self, parent):
    if self != None:
        return self
    return parent

def _child_flavor_impl(ctx: AnalysisContext) -> list[Provider]:
    parent = ctx.attrs.parent[FlavorInfo]
    flavor = FlavorInfo(
        default_build_appliance = _overridden_attr(ctx.attrs.default_build_appliance, parent.default_build_appliance),
        dnf_info = FlavorDnfInfo(
            default_excluded_rpms = _overridden_attr(ctx.attrs.default_dnf_excluded_rpms, parent.dnf_info.default_excluded_rpms),
            default_extra_repo_set = _overridden_attr(ctx.attrs.default_dnf_extra_repo_set, parent.dnf_info.default_extra_repo_set),
            default_repo_set = _overridden_attr(ctx.attrs.default_dnf_repo_set, parent.dnf_info.default_repo_set),
            default_versionlock = _overridden_attr(ctx.attrs.default_dnf_versionlock, parent.dnf_info.default_versionlock),
            reflink_flavor = _overridden_attr(ctx.attrs.rpm_reflink_flavor, parent.dnf_info.reflink_flavor),
        ),
    )
    return [
        flavor,
        DefaultInfo(sub_targets = {
            "default_build_appliance": flavor.default_build_appliance.providers,
            "default_versionlock": [DefaultInfo(flavor.dnf_info.default_versionlock)],
        }),
    # @oss-disable
    ] # @oss-enable

_child_flavor = rule(
    impl = _child_flavor_impl,
    attrs = {
        "parent": attrs.dep(providers = [FlavorInfo]),
    } | {
        name: attrs.option(attr, default = None)
        for name, attr in _flavor_attrs.items()
    },
)

child_flavor = rule_with_default_target_platform(_child_flavor)
