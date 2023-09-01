# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "FlavorDnfInfo", "FlavorInfo", "LayerInfo")
# @oss-disable
load("//antlir/bzl:build_defs.bzl", "alias", "config")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")

_flavor_attrs = {
    "default_build_appliance": attrs.dep(providers = [LayerInfo]),
    "default_dnf_excluded_rpms": attrs.list(
        attrs.string(),
        default = [],
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
    # @oss-enable ]

_flavor = rule(
    impl = _impl,
    attrs = _flavor_attrs,
)

def flavor(
        name: str,
        flavored_build_appliance: str,
        # Force the flavor author to say that their flavor does not support
        # reflink to make it impossible to forget
        rpm_reflink_flavor: str | None,
        **kwargs):
    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    # Ideally this would be a subtarget, but then it would be a circular dependency
    alias(
        name = name + ".build-appliance",
        actual = flavored_build_appliance,
        visibility = kwargs.get("visibility", None),
    )
    return _flavor(
        name = name,
        rpm_reflink_flavor = rpm_reflink_flavor,
        **kwargs
    )

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
    # @oss-enable ]

_child_flavor = rule(
    impl = _child_flavor_impl,
    attrs = {
        "parent": attrs.dep(providers = [FlavorInfo]),
    } | {
        name: attrs.option(attr, default = None)
        for name, attr in _flavor_attrs.items()
    },
)

def child_flavor(
        name: str,
        parent: str,
        flavored_build_appliance: str | None = None,
        **kwargs):
    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    # Ideally this would be a subtarget, but then it would be a circular dependency
    if flavored_build_appliance:
        alias(
            name = name + ".build-appliance",
            actual = flavored_build_appliance,
            visibility = kwargs.get("visibility", None),
        )
    else:
        alias(
            name = name + ".build-appliance",
            actual = parent + ".build-appliance",
            visibility = kwargs.get("visibility", None),
        )
    return _child_flavor(
        name = name,
        parent = parent,
        **kwargs
    )
