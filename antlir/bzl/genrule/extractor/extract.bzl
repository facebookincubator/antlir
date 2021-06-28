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

An important note: If you are using buck compiled binaries, you *must*
use `image.install(...)` to insert them into your `source` layer and *not*
`image.install_buck_runnable(...)`. This is the exact opposite of the
suggested usage in the rest of the API, and here's why:

If `image.install_buck_runnable(...)` is built in the case where
`REPO_CFG.artifacts_require_repo == True`, then what is *actually*
installed into the target `image.layer` is a shell script that exec's
the *actual* binary from the `buck-out` path contained somewhere in the
repo. Since this is a shell script, we can't easily do ELF extraction
without doing some nasty file parsing, and right now we'd rather not
do that because it's possible that we can fix it correctly and `image.install`
is good enough.

The caveat to using `image.install` is that the compiled binary must be
*mostly* static (meaning it only depends on the glibc it is compiled against)
__or__ it must *not* use relative references to shared objects that cannot
be resolved in the `source`. These restrictions, especially the last one, are
pretty difficult to reason about in advance of just trying to perform an
extraction.  As a result, the best advice we can give at this point is to
only use `rust_binary` with the `link_style = "static"` option for any
binary target you want to use via the extractor.

Future: Fixing the main problem with `image.install_buck_runnable` would likely
involve parsing the generated bash script and extracting the "real" path of the
binary.

We examined using symlinks to make parsing not required, but we can't
really use a symlink because many/most binaries that are built with buck using
this symlink farm structure rely on the fact that $0 resolves to a path within
`buck-out`.  So if we use a symlink it would break this assumption.  This
doesn't mean that parsing is the only option, but the most obvious alternative
(symlinks) won't work with the current way buck builds these binaries.
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
        # The flavor of the extracted layer
        flavor,
        # A list of binaries to extract from the source,
        binaries,
        # The root destination path to clone the extracted
        # files into.
        dest = "/"):
    binaries = binaries or []

    layer_hash = sha256_b64(normalize_target(source) + " ".join(binaries) + dest)
    base_extract_layer = "image-extract-setup--{}".format(layer_hash)
    image.layer(
        name = base_extract_layer,
        parent_layer = source,
        features = [
            image.ensure_dirs_exist("/output"),
            image.install_buck_runnable("//antlir/bzl/genrule/extractor:extract", "/extract"),
        ],
        visibility = [],
        flavor = flavor,
    )
    extract_parent_layer = ":" + base_extract_layer

    binaries_args = []
    for binary in binaries:
        binaries_args.extend([
            "--binary",
            binary,
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
        flavor = flavor,
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
