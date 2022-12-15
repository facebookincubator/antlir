# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:layer_info.bzl", "LayerInfo")
load(":feature_info.bzl", "InlineFeatureInfo")

def parent_layer(
        *,
        layer: str.type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "parent_layer",
        deps = {
            "layer": layer,
        },
        kwargs = {},
    )

def parent_layer_to_json(
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    if LayerInfo not in deps["layer"]:
        fail("'{}' is not an image layer".format(deps["layer"].label))
    return {
        "layer": deps["layer"].label,
    }
