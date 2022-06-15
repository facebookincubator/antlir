# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:wrap_runtime_deps.bzl", "maybe_wrap_executable_target")
load("//antlir/bzl/image/feature:tarball.shape.bzl", "tarball_t")
load(":helpers.bzl", "normalize_target_and_mark_path")
load(":image_source.shape.bzl", "image_source_t")
load(":rules.bzl", "maybe_add_feature_rule")

def _generate_source_dict_and_normalize_target(source):
    source_dict = shape.as_dict_shallow(image_source(maybe_export_file(source)))
    normalized_target = None
    if source_dict.get("source"):
        normalized_target = normalize_target_and_mark_path(
            source_dict,
            "source",
        )
    elif source_dict.get("generator"):
        _was_wrapped, source_dict["generator"] = maybe_wrap_executable_target(
            target = source_dict["generator"],
            wrap_suffix = "image_source_wrap_generator",
            visibility = [],  # Not visible outside of project
            # Generators run at build-time, that's the whole point.
            runs_in_build_steps_causes_slow_rebuilds = True,
        )
        normalized_target = normalize_target_and_mark_path(
            source_dict,
            "generator",
        )

    return source_dict, normalized_target

def _generate_shape(source_dict, dest, force_root_ownership):
    return shape.new(
        tarball_t,
        force_root_ownership = force_root_ownership,
        into_dir = dest,
        source = shape.new(image_source_t, **source_dict),
    )

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

    source_dict, normalized_target = _generate_source_dict_and_normalize_target(
        source,
    )

    return maybe_add_feature_rule(
        name = "tarball",
        key = "tarballs",
        include_in_target_name = {
            "dest": dest,
            "source": source,
        },
        feature_shape = _generate_shape(
            source_dict,
            dest,
            force_root_ownership,
        ),
        deps = [normalized_target],
    )
