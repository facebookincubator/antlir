# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2 = "feature")
load("//antlir/bzl:image_source.bzl", "image_source_to_buck2_src")

def feature_tarball(source, dest, force_root_ownership = False):
    """
`feature.tarball("files/xyz.tar", "/a/b")` extracts tarball located at `files/xyz.tar` to `/a/b` in the image --
- `source` is one of:
    - an `image.source` (docs in `image_source.bzl`), or
    - the path of a target outputting a tarball target path,
    e.g. an `export_file` or a `genrule`
- `dest` is the destination of the unpacked tarball in the image.
    This is an image-absolute path to a directory that must be created
    by another `feature_new` item.
    """
    buck2_src = image_source_to_buck2_src(source)

    return antlir2.tarball(
        src = buck2_src,
        into_dir = dest,
        force_root_ownership = force_root_ownership,
    )
