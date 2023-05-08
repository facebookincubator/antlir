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

def layer_dep_analyze(layer: "dependency", _implicit_from_antlir1: bool.type = False) -> layer_dep.type:
    """
    Serialize a Layer dependency to a subset of LayerInfo that can be serialized
    and passed to antlir2
    """

    # If we're analyzing this as a result of an implicit conversion from
    # antlir1, there is a good chance that the 'layer' is not actually antlir2
    # layer, in which case we obviously don't have this information.
    # Rather than fail the buck analysis, we can return a marker that will fail
    # in the antlir2 compiler. This way the dependency layer only needs to be
    # fixed when building an image.layer with this feature, not when simply
    # building the feature.
    if LayerInfo not in layer:
        if _implicit_from_antlir1:
            return layer_dep(
                label = layer.label,
                appears_to_be_antlir1_layer = True,
                depgraph = None,
                subvol_symlink = None,
                mounts = None,
            )

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
