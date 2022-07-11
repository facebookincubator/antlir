# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:tarball.shape.bzl", "tarball_t")
load(":image_source.shape.bzl", "image_source_t")
load(":rules.bzl", "maybe_add_feature_rule")
load(":source_dict_helper.bzl", "normalize_target_and_mark_path_in_source_dict")

def feature_tarball(source, dest, force_root_ownership = False):
    """
    `feature.tarball("files/xyz.tar", "/a/b")` extracts tarball located at
    `files/xyz.tar` to `/a/b` in the image --
    - `source` is one of:
        - an `image.source` (docs in `image_source.bzl`), or
        - the path of a target outputting a tarball target path,
        e.g. an `export_file` or a `genrule`
    - `dest` is the destination of the unpacked tarball in the image.
        This is an image-absolute path to a directory that must be created
        by another `feature_new` item.
    """
    source_dict = shape.as_dict_shallow(image_source(maybe_export_file(source)))
    source_dict, normalized_target = \
        normalize_target_and_mark_path_in_source_dict(source_dict)

    return maybe_add_feature_rule(
        name = "tarball",
        key = "tarballs",
        include_in_target_name = {
            "dest": dest,
            "source": source_dict,
        },
        feature_shape = tarball_t(
            force_root_ownership = force_root_ownership,
            into_dir = dest,
            source = shape.new(image_source_t, **source_dict),
        ),
        deps = [normalized_target],
    )
