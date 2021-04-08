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

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def _buck_binary_tmp_dst(real_dst):
    if paths.is_absolute(real_dst):
        real_dst = paths.normalize(real_dst)[1:]
    return paths.join("/buck-binaries", real_dst.replace("/", "_"))

def _extract(
        # The layer from which to extract the binary and deps
        source,
        # A list of binaries to extract from the source,
        binaries = None,
        # The root destination path to clone the extracted
        # files into.
        dest = "/",
        # Buck binaries to extract. dict of target -> dest path
        buck_binaries = None):
    if not binaries and not buck_binaries:
        fail("at least one of 'binaries' and 'buck_binaries' must be given")
    binaries = binaries or []
    buck_binaries = buck_binaries or {}

    layer_hash = sha256_b64(normalize_target(source) + " ".join(binaries) + dest)
    base_extract_layer = "image-extract-setup--{}".format(layer_hash)
    image.layer(
        name = base_extract_layer,
        parent_layer = source,
        features = [
            image.ensure_dirs_exist("/output"),
            image.install_buck_runnable("//antlir/bzl/genrule/extractor:extract", "/extract"),
            image.ensure_dirs_exist("/buck-binaries"),
        ] + [
            # when artifacts_require_repos = False, these are not used and are
            # instead replaced with the absolute path as given by `$(exe)`
            image.install(target, _buck_binary_tmp_dst(dst), mode = "a+rx")
            for target, dst in buck_binaries.items()
        ],
        visibility = [],
    )
    extract_parent_layer = ":" + base_extract_layer

    binaries_args = []
    for binary in binaries:
        binaries_args.extend([
            "--binary",
            binary,
        ])

    for target, dst in buck_binaries.items():
        if REPO_CFG.artifacts_require_repo:
            # If buck built binaries require repo artifacts (ie @mode/dev),
            # then give the extractor the full path to executable so it can
            # properly parse it.
            binaries_args.extend([
                "--buck-binary",
                "$(exe {}):{}".format(target, dst),
            ])
        else:
            # Otherwise, the binaries as installed above should be sufficient to
            # extract by themselves.
            binaries_args.extend([
                "--buck-binary",
                "{}:{}".format(_buck_binary_tmp_dst(dst), dst),
            ])

    work_layer = "image-extract-work--{}".format(layer_hash)
    image.genrule_layer(
        name = work_layer,
        rule_type = "extract",
        parent_layer = extract_parent_layer,
        user = "root",
        cmd = [
            "/extract",
            "--src-dir",
            "/",
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
# extracted.
# NOTE: parent_layer is currently not allowed, because extracting a buck-built
# fbcode binary while using any parent_layer with the /usr/local/fbcode host
# mount is broken due to protected paths causing image.clone to fail. If this
# is needed in the future, it can be resolved then.
def _source_layer(name, **kwargs):
    if "parent_layer" in kwargs:
        fail("not allowed here, see above comment", attr = "parent_layer")
    image.layer(
        name = name,
        parent_layer = REPO_CFG.artifact["extractor.common_deps"],
        **kwargs
    )

# Eventually this would (hopefully) be provided as a top-level
# api within `//antlir/bzl:image.bzl`, so lets start namespacing
# here.
extract = struct(
    extract = _extract,
    source_layer = _source_layer,
)
