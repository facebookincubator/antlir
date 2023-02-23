# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
WARNING: you probably don't actually want this
extract.bzl exists for very stripped down environments (for example, building
an initrd) that need a binary (most likely from an RPM) and its library
dependencies. In almost every case _other_ than building an initrd, you
either want `feature.rpms_install` or `feature.install_buck_runnable`

If you're still here, `extract.extract` works by parsing the ELF information
in the given binaries.
It then clones the binaries and any .so's they depend on from the source
layer into the destination layer. The actual clone is very unergonomic at
this point, and it is recommended to batch all binaries to be extracted into
a single call to `extract.extract`.

This new-and-improved version of extract is capable of extracting buck-built
binaries without first installing them into a layer.
"""

load("//antlir/staging/antlir2:antlir2_layer_info.bzl", "LayerInfo")
load(":feature_info.bzl", "InlineFeatureInfo")

def extract_from_layer(
        layer: str.type,
        binaries: [str.type]) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "extract",
        deps = {
            "layer": layer,
        },
        kwargs = {
            "binaries": binaries,
            "source": "layer",
        },
    )

def extract_buck_binary(
        src: str.type,
        dst: str.type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "extract",
        # include in deps so we can look at the providers
        deps = {"src": src},
        # also add it to sources so it gets materialized
        sources = {"src": src},
        kwargs = {
            "dst": dst,
            "source": "buck",
        },
    )

def extract_to_json(
        source: str.type,
        deps: {str.type: "dependency"},
        sources: [{str.type: "artifact"}, None] = None,
        binaries: [[str.type], None] = None,
        src: [str.type, None] = None,
        dst: [str.type, None] = None) -> {str.type: ""}:
    if source == "layer":
        return {
            "binaries": binaries,
            "layer": deps["layer"][LayerInfo],
            "source": "layer",
        }
    elif source == "buck":
        src = deps["src"]
        if RunInfo not in src:
            fail("'{}' does not appear to be a binary".format(src))
        return {
            "dst": dst,
            "source": "buck",
            "src": sources["src"],
        }
    else:
        fail("invalid extract source '{}'".format(source))
