# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

layer_dep = record(
    depgraph = "artifact",
    label = "label",
    subvol_symlink = "artifact",
    mounts = ["mount_record"],
)

def layer_dep_analyze(layer: "dependency") -> layer_dep.type:
    """
    Serialize a Layer dependency to a subset of LayerInfo that can be serialized
    and passed to antlir2
    """
    info = layer[LayerInfo]
    return layer_dep(
        depgraph = info.depgraph,
        label = info.label,
        subvol_symlink = info.subvol_symlink,
        mounts = info.mounts,
    )
