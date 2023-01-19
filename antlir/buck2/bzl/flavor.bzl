# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.shape.bzl", "flavor_config_t")
load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl:types.bzl", "types")
load(":distro.bzl", "DistroInfo")

types.lint_noop(flavor_config_t)

"""
Describe an image flavor. Basically, this is a set of defaults for image builds,
including the build appliance and repository snapshot(s).
"""
FlavorInfo = provider(fields = {
    "build_appliance": "Layer that is used as the default build appliance for images of this flavor",
    "distro": "DistroInfo provider",
})

def _flavor_impl(ctx: "context") -> ["provider"]:
    flavor_json = ctx.actions.write_json("flavor.json", {
        "label": ctx.label,
    })

    return [
        FlavorInfo(
            distro = ctx.attrs.distro,
            build_appliance = ctx.attrs.build_appliance,
        ),
        DefaultInfo(default_outputs = [flavor_json]),
    ]

flavor = rule(
    impl = _flavor_impl,
    attrs = {
        "build_appliance": attrs.option(attrs.dep(providers = [
            # this should be BuildApplianceInfo, but not everything is a proper rule yet
            # load(":build_appliance.bzl", "BuildApplianceInfo")
            # BuildApplianceInfo
        ]), default = None),
        "distro": attrs.option(attrs.dep(providers = [DistroInfo])),
    },
)

def flavor_to_config(flavor: ["dependency", str.type]) -> types.shape(flavor_config_t):
    # TODO(T139523690) this should be coming from the provider
    if type(flavor) == "dependency":
        flavor = flavor.label.name
    if ":" in flavor:
        _, flavor = flavor.rsplit(":")
    return flavor_helpers.get_flavor_config(flavor, flavor_config_override = None)

def coerce_to_flavor_label(flavor: str.type) -> str.type:
    if ":" not in flavor:
        return "//antlir/facebook/flavor:" + flavor
    return flavor
