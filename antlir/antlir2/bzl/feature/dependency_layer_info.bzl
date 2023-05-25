# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

layer_dep = record(
    label = "label",
    depgraph = ["artifact", None],
    subvol_symlink = ["artifact", None],
    mounts = [["mount_record"], None],
    # We can't do anything useful with this, but if recording that this appears
    # to be an antlir1 layer will be useful for ripping this out later
    appears_to_be_antlir1_layer = [bool.type, None],
)

def layer_dep_analyze(layer: "dependency") -> layer_dep.type:
    """
    Serialize a Layer dependency to a subset of LayerInfo that can be serialized
    and passed to antlir2
    """
    if LayerInfo not in layer:
        # antlir2 on the other hand should fail
        fail("'{}' is not an antlir2 image layer".format(layer.label))
    info = layer[LayerInfo]
    return layer_dep(
        depgraph = info.depgraph,
        label = info.label,
        subvol_symlink = info.subvol_symlink,
        mounts = info.mounts,
        appears_to_be_antlir1_layer = False,
    )
