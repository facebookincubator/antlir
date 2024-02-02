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

An important note: If you are using buck compiled binaries, you *must*
use `feature.install(...)` to insert them into your `source` layer and *not*
`feature.install_buck_runnable(...)`. This is the exact opposite of the
suggested usage in the rest of the API, and here's why:

If `feature.install_buck_runnable(...)` is built in the case where
`REPO_CFG.artifacts_require_repo == True`, then what is *actually*
installed into the target `image.layer` is a shell script that exec's
the *actual* binary from the `buck-out` path contained somewhere in the
repo. Since this is a shell script, we can't easily do ELF extraction
without doing some nasty file parsing, and right now we'd rather not
do that because it's possible that we can fix it correctly and `feature.install`
is good enough.

The caveat to using `feature.install` is that the compiled binary must be
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

IMPORTANT NOTE:
extract.extract will first `image_remove` all file paths to be exported,
then `image_clone` the same paths to destination layer.

The reason for this is to avoid conflicts on .so files that were potentially already
exported by a parent layer which also includes an extract.extract feature.
"""

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:antlir2_shim.bzl", "antlir2_shim")
load("//antlir/bzl:build_defs.bzl", "is_buck2")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def _extract(
        # The layer from which to extract the binary and deps
        source,
        # A list of binaries to extract from the source,
        binaries,
        # The root destination path to clone the extracted
        # files into.
        dest = "/",
        antlir2 = None):
    if dest != "/":
        fail("extract(dest='/') is no longer allowed")
    binaries = binaries or []
    normalized_source = normalize_target(source)
    name = sha256_b64(normalized_source + " ".join(binaries) + dest)

    if antlir2_shim.upgrade_or_shadow_feature(
        name = name,
        antlir2 = antlir2,
        fn = antlir2_shim.getattr_buck2(antlir2_feature, "new"),
        features = [
            antlir2_feature.extract_from_layer(
                binaries = binaries,
                layer = source,
            ) if is_buck2() else None,
        ],
        visibility = [],
    ) != "upgrade":
        fail("antlir1 is dead")

    return normalize_target(":" + name)

# Eventually this would (hopefully) be provided as a top-level
# api within `//antlir/bzl:image.bzl`, so lets start namespacing
# here.
extract = struct(
    extract = _extract,
)
