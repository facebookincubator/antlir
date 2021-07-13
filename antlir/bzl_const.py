# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.bzl_const_t import data as BZL_CONST
from antlir.config import repo_config


# IMPORTANT: Keep in sync with `bzl/image/feature/new.bzl`
def feature_target_name(name, flavor):
    name += BZL_CONST.PRIVATE_feature_suffix

    # When a feature is declared, it doesn't know the version set of the
    # layer that will use it, so we normally declare all possible variants.
    # This is only None when there are no version sets in use.
    version_set_path = repo_config().flavor_to_config[flavor].version_set_path
    if version_set_path != BZL_CONST.version_set_allow_all_versions:
        name += "__flavor__" + flavor
    return name


# IMPORTANT: Keep in sync with `bzl/compile_image_features.bzl`
def feature_for_layer(layer_name, flavor):
    assert (
        BZL_CONST.PRIVATE_feature_suffix not in layer_name
    ), f"Got feature target instead of layer: {layer_name}"
    return feature_target_name(
        layer_name + BZL_CONST.layer_feature_suffix, flavor
    )
