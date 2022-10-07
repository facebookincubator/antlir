# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl/feature:feature.bzl", "FeatureInfo", "feature")
load(":flatten.bzl", "flatten")
load(":flavor.bzl", "FlavorInfo")
load(":layer_info.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    if ctx.attrs.parent_layer and ctx.attrs.flavor:
        # this could technically be a check that guarantees they match, but why
        # even let people provide redundant information?
        fail("flavor should not be set in combination with parent_layer")
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
        # Accept features as a mix of target labels, inline features or lists of
        # the same. Add more levels of nesting as necessary
        features: [["InlineFeatureInfo", str.type, ["InlineFeatureInfo"]]],
        **kwargs):
    feature_target = name + "--features"
    feature(
        name = feature_target,
        features = flatten(features),
    )
    feature_target = ":" + feature_target

    return _layer(
        name = name,
        features = feature_target,
        **kwargs
    )
