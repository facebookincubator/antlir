# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "FlavorDnfInfo", "FlavorInfo", "LayerInfo")
load("//antlir/bzl:build_defs.bzl", "config")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")

def _impl(ctx: "context") -> ["provider"]:
    return [
        FlavorInfo(
            label = ctx.label,
            default_build_appliance = ctx.attrs.default_build_appliance,
            dnf_info = FlavorDnfInfo(
                default_repo_set = ctx.attrs.default_dnf_repo_set,
                default_versionlock = ctx.attrs.default_dnf_versionlock,
                default_excluded_rpms = ctx.attrs.default_dnf_excluded_rpms,
            ),
        ),
        DefaultInfo(),
    ]

_flavor = rule(
    impl = _impl,
    attrs = {
        "default_build_appliance": attrs.dep(providers = [LayerInfo]),
        "default_dnf_excluded_rpms": attrs.list(attrs.string(), default = []),
        "default_dnf_repo_set": attrs.dep(providers = [RepoSetInfo]),
        "default_dnf_versionlock": attrs.option(attrs.source(), default = None),
    },
)

def flavor(**kwargs):
    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform
    return _flavor(**kwargs)
