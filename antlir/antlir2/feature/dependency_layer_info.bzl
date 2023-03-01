# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2:antlir2_layer_info.bzl", "LayerInfo")

def layer_dep_to_json(layer: "dependency") -> {str.type: ""}:
    """
    Serialize a Layer dependency to a subset of LayerInfo that can be serialized
    and passed to antlir2
    """
    info = layer[LayerInfo]
    return {
        "depgraph": info.depgraph,
        "label": layer.label,
        "subvol_symlink": info.subvol_symlink,
    }