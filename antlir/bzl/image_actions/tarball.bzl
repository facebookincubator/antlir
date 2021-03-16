# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/bzl:target_tagger.bzl",
    "image_source_as_target_tagged_shape",
    "new_target_tagger",
    "target_tagged_image_source_shape",
    "target_tagger_to_feature",
)

tarball_t = shape.shape(
    force_root_ownership = shape.field(bool, optional = True),
    into_dir = shape.path(),
    source = target_tagged_image_source_shape,
)

def image_tarball(source, dest, force_root_ownership = False):
    """
`image.tarball("files/xyz.tar", "/a/b")` extracts tarball located at `files/xyz.tar` to `/a/b` in the image --
- `source` is one of:
    - an `image.source` (docs in `image_source.bzl`), or
    - the path of a target outputting a tarball target path,
    e.g. an `export_file` or a `genrule`
- `dest` is the destination of the unpacked tarball in the image.
    This is an image-absolute path to a directory that must be created
    by another `image_feature` item.
    """
    target_tagger = new_target_tagger()
    tarball = shape.new(
        tarball_t,
        force_root_ownership = force_root_ownership,
        into_dir = dest,
        source = image_source_as_target_tagged_shape(
            target_tagger,
            maybe_export_file(source),
        ),
    )

    return target_tagger_to_feature(
        target_tagger,
        items = struct(tarballs = [tarball]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:tarball"],
    )
