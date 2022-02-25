# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.bzl_const_t import data as BZL_CONST


# IMPORTANT: Keep in sync with `bzl/image/feature/new.bzl`
def feature_target_name(name):
    return name + BZL_CONST.PRIVATE_feature_suffix


# IMPORTANT: Keep in sync with `bzl/compile_image_features.bzl`
def feature_for_layer(layer_name):
    assert (
        BZL_CONST.PRIVATE_feature_suffix not in layer_name
    ), f"Got feature target instead of layer: {layer_name}"
    return feature_target_name(layer_name + BZL_CONST.layer_feature_suffix)


def hostname_for_compiler_in_ba():
    return BZL_CONST.hostname_for_compiler_in_ba
