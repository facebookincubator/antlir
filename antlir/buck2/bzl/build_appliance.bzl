# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Build appliances are a special use case of images, where certain layers are used
to build customer image layers.
Build appliance images have useful tools and data installed in them to enable
things like RPM installation, image output packaging, etc.

Since they are an important primitive in the Antlir ecosystem, yet are still
image layers themselves, they come with some unique challenges, mainly around
flavors and bootstrapping.

At some point, the build appliance is bootstrapped from an image package from a
blobstore, and it may have features added on top before actually being used for
image builds.

Unfortunately, there exists a (weak) circular dependency since a Flavor has a
default build appliance, but the build appliance ideally would have the same
flavor in the target graph. To break this circular dependency, build appliance
images have another provider, BuildApplianceInfo that can be used in place of it
having a proper FlavorInfo in the layer target itself.
"""

load(":distro.bzl", "DistroInfo")
load(":layer_info.bzl", "LayerInfo")

BuildApplianceInfo = provider(fields = {
    "distro": "DistroInfo provider",
    "flavor_label": "Label pointing to a flavor target",
})

def _build_appliance_impl(ctx: "context") -> ["provider"]:
    layer = ctx.attrs.layer[LayerInfo]
    if layer.flavor:
        fail("build_appliance layers must not have a flavor set, since it will be removed")
    return [
        LayerInfo(
            default_mountpoint = layer.default_mountpoint,
            features = [],
            parent_layer = None,
            flavor = None,
        ),
        BuildApplianceInfo(
            distro = ctx.attrs.distro,
            flavor_label = ctx.attrs.flavor,
        ),
        DefaultInfo(),
    ]

build_appliance = rule(
    impl = _build_appliance_impl,
    attrs = {
        "distro": attrs.dep(providers = [DistroInfo]),
        # Point to the real flavor that this build appliance supports, but only
        # as a label to avoid a circular dep
        "flavor": attrs.label(),
        "layer": attrs.dep(providers = [LayerInfo]),
    },
)
