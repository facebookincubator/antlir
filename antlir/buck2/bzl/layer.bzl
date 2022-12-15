# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl/feature:feature.bzl", "FeatureInfo", "feature")
load("//antlir/bzl:flatten.bzl", "flatten")
load(":build_appliance.bzl", "BuildApplianceInfo")
load(":flavor.bzl", "FlavorInfo")
load(":layer_info.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    # Providing a flavor in combination with parent_layer is not necessary, but
    # can be used to add guarantees that the parent isn't swapped out to a new
    # flavor without forcing this child to acknowledge the change in cases where
    # that might be desirable.
    if ctx.attrs.parent_layer and ctx.attrs.flavor:
        # see build_appliance.bzl for why this special case is necessary
        if BuildApplianceInfo in ctx.attrs.parent_layer:
            parent_flavor_label = ctx.attrs.parent_layer[BuildApplianceInfo].flavor_label
        else:
            parent_flavor_label = ctx.attrs.parent_layer[LayerInfo].flavor.label
        if parent_flavor_label != ctx.attrs.flavor.label:
            fail("parent_layer flavor is {}, but this layer is trying to use {}".format(
                parent_flavor_label,
                ctx.attrs.flavor.label,
            ))
    if not ctx.attrs.flavor and not ctx.attrs.parent_layer:
        fail("flavor is required with no parent_layer")

    features = ctx.attrs.features[FeatureInfo]
    return [
        DefaultInfo(
            default_outputs = [],
        ),
    ]

_layer = rule(
    impl = _impl,
    attrs = {
        "features": attrs.dep(providers = [FeatureInfo]),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo])),
        "parent_layer": attrs.option(attrs.dep(providers = [LayerInfo])),
    },
)

def layer(
        *,
        name: str.type,
        # Features does not have a direct type hint, but it is still validated
        # by a type hint inside feature.bzl. Feature targets or
        # InlineFeatureInfo providers are accepted, at any level of nesting
        features = [],
        **kwargs):
    feature_target = name + "--features"
    feature(
        name = feature_target,
        features = flatten.flatten(features, item_type = ["InlineFeatureInfo", str.type]),
        visibility = [":" + name],
    )
    feature_target = ":" + feature_target

    return _layer(
        name = name,
        features = feature_target,
        **kwargs
    )
