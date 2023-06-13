# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "FlavorDnfInfo", "FlavorInfo", "LayerInfo")
load("//antlir/bzl:build_defs.bzl", "alias", "config")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")

def _impl(ctx: "context") -> ["provider"]:
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
        }),
    ]

_flavor = rule(
    impl = _impl,
    attrs = {
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
    },
)

def flavor(
        name: str.type,
        flavored_build_appliance: str.type,
        # Force the flavor author to say that their flavor does not support
        # reflink to make it impossible to forget
        rpm_reflink_flavor: [str.type, None],
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
