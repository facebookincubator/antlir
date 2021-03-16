# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
WARNING: you probably don't actually want this
extract.bzl exists for very stripped down environments (for example, building
an initrd) that need a binary (most likely from an RPM) and its library
dependencies. In almost every case _other_ than building an initrd, you
either want `image.rpms_install` or `image.install_buck_runnable`

If you're still here, `extract.extract` works by parsing the ELF information
in the given binaries.
It then clones the binaries and any .so's they depend on from the source
layer into the destination layer. The actual clone is very unergonomic at
this point, and it is recommended to batch all binaries to be extracted into
a single call to `extract.extract`.
"""

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

DEFAULT_EXTRACT_TOOLS_LAYER = "//antlir/bzl/foreign/extractor:extract-tools-layer"

def _extract(
        # The layer from which to extract the binary and deps
        source,
        # A list of binaries to extract from the source,
        binaries = None,
        # The root destination path to clone the extracted
        # files into.
        dest = "/",
        # Buck binaries to extract. dict of target -> dest path
        buck_binaries = None,
        # The tool layer
        tool_layer = DEFAULT_EXTRACT_TOOLS_LAYER):
    if not binaries and not buck_binaries:
        fail("at least one of 'binaries' and 'buck_binaries' must be given")
    if not binaries:
        binaries = []
    if not buck_binaries:
        buck_binaries = {}

    layer_hash = sha256_b64(normalize_target(source) + " ".join(binaries) + dest)
    base_extract_layer = "image-extract-setup--{}".format(layer_hash)
    image.layer(
        name = base_extract_layer,
        parent_layer = tool_layer,
        features = [
            image.layer_mount(
                source,
                "/source",
            ),
        ],
        visibility = [],
    )

    binaries_args = []
    for binary in binaries:
        binaries_args.extend([
            "--binary",
            "{}:{}".format(binary, binary),
        ])
    for target, dst in buck_binaries.items():
        binaries_args.extend([
            "--buck-binary",
            "$(exe {}):{}".format(target, dst),
        ])

    work_layer = "image-extract-work--{}".format(layer_hash)
    image.genrule_layer(
        name = work_layer,
        rule_type = "image_extract",
        parent_layer = ":" + base_extract_layer,
        user = "root",
        cmd = [
            "/extract",
            "--src-dir",
            "/source",
            "--dst-dir",
            "/output",
        ] + binaries_args,
        antlir_rule = "user-internal",
    )

    # The output is an image.clone feature that clones
    # the extracted files into `dest`
    return image.clone(
        ":" + work_layer,
        "output/",
        dest,
    )

# Helper to create a layer to use as 'source' for 'extract.extract', that
# already has dependencies likely to be required by the binaries being
# extracted
def _source_layer(name, features):
    image.layer(
        name = name,
        parent_layer = REPO_CFG.artifact["extractor.common_deps"],
        features = features,
    )

# Eventually this would (hopefully) be provided as a top-level
# api within `//antlir/bzl:image.bzl`, so lets start namespacing
# here.
extract = struct(
    extract = _extract,
    source_layer = _source_layer,
)
