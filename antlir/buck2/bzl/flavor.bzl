# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_appliance.bzl", "BuildApplianceInfo")
load(":distro.bzl", "DistroInfo")

"""
Describe an image flavor. Basically, this is a set of defaults for image builds,
including the build appliance and repository snapshot(s).
"""
FlavorInfo = provider(fields = {
    "build_appliance": "Layer that is used as the default build appliance for images of this flavor",
    "distro": "DistroInfo provider",
})

def _flavor_impl(ctx: "context") -> ["provider"]:
    return [
        FlavorInfo(
            distro = ctx.attrs.distro,
            build_appliance = ctx.attrs.build_appliance,
        ),
        DefaultInfo(default_outputs = []),
    ]

flavor = rule(
    impl = _flavor_impl,
    attrs = {
        "build_appliance": attrs.option(attrs.dep(providers = [BuildApplianceInfo]), default = None),
        "distro": attrs.dep(providers = [DistroInfo]),
    },
)
