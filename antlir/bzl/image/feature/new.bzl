# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:antlir2_shim.bzl", "antlir2_shim")
load("//antlir/bzl:build_defs.bzl", "get_visibility")
load("//antlir/bzl:flatten.bzl", "flatten")

def feature_new(
        name,
        features,
        visibility = None,
        # This is used when a user wants to declare a feature
        # that is not available for all flavors in REPO_CFG.flavor_to_config.
        # An example of this is the internal feature in `image_layer.bzl`.
        flavors = None,
        antlir2 = None,
        # If set, will be directly used as antlir2 features, else we'll attempt to
        # derive them implicitly from `features`
        antlir2_features = None):
    antlir2_feature.new(
        name = name,
        features = antlir2_features or [f if types.is_string(f) else getattr(f, "antlir2_feature", f) for f in flatten.flatten(features)],
        visibility = get_visibility(visibility),
    )
    if not antlir2_shim.should_upgrade_feature():
        fail("antlir1 is dead")
