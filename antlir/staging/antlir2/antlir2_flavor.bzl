# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "config")
load("//antlir/rpm/dnf2buck:repo.bzl", "RepoSetInfo")
load(":antlir2_layer_info.bzl", "LayerInfo")

FlavorInfo = provider(fields = {
    "default_build_appliance": "The default build_appliance to use on images of this flavor",
    "default_rpm_repo_set": "The default set of rpm repos available to images of this flavor",
    "label": "The buck label for this flavor",
})

def _impl(ctx: "context") -> ["provider"]:
    return [
        FlavorInfo(
            label = ctx.label,
            default_build_appliance = ctx.attrs.default_build_appliance[LayerInfo],
            default_rpm_repo_set = ctx.attrs.default_rpm_repo_set[RepoSetInfo],
        ),
        DefaultInfo(),
    ]

_antlir2_flavor = rule(
    impl = _impl,
    attrs = {
        "default_build_appliance": attrs.dep(providers = [LayerInfo]),
        "default_rpm_repo_set": attrs.dep(providers = [RepoSetInfo]),
    },
)

def antlir2_flavor(**kwargs):
    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform
    return _antlir2_flavor(**kwargs)
