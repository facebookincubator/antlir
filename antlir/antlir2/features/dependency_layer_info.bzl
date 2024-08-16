# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:mount_types.bzl", "mount_record")

layer_dep = record(
    label = Label,
    facts_db = Artifact,
    contents = struct,
    mounts = list[mount_record],
)

def layer_dep_analyze(layer: Dependency) -> layer_dep:
    """
    Serialize a Layer dependency to a subset of LayerInfo that can be serialized
    and passed to antlir2
    """
    if LayerInfo not in layer:
        fail("'{}' is not an antlir2 image layer".format(layer.label))
    info = layer[LayerInfo]
    contents = struct(overlayfs = info.contents.overlayfs) if info.contents.overlayfs else struct(subvol_symlink = info.contents.subvol_symlink)
    return layer_dep(
        label = info.label,
        facts_db = info.facts_db,
        contents = contents,
        mounts = info.mounts,
    )
