# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "clean_target_name")
load(
    "//antlir/bzl/image/feature:new.bzl",
    "PRIVATE_DO_NOT_USE_feature_target_name",
)

def generate_feature_target_name(
        name,
        key,
        feature_shape,
        include_in_name = None):
    """
    Generate a memoization-safe target name for features.

    - `name`: type of feature
    - `feature_shape`: feature shape generated in `feature_{name}`
    - `include_in_name`: feature arguments that are wanted in their original
        form (perhaps for debugging purposes).
    """

    if include_in_name:
        # only include collections if they are not empty in name
        include_in_name = sorted(
            [
                item
                for item in include_in_name.items()
                if item[1] or item[1] == 0
            ],
        )

    include_in_name_str = (
        "".join(
            [
                "{arg_name}_{arg}_".format(
                    arg_name = arg_name.upper(),
                    arg = arg,
                )
                for arg_name, arg in include_in_name
            ],
        )[:-1] if include_in_name else ""
    )

    # if list of feature shapes is provided, hash all shapes, join their hashes
    # and then hash that final string to obtain `shape_hash`.
    if types.is_list(feature_shape):
        shape_hash = sha256_b64("_".join(sorted([
            shape.hash(s)
            for s in feature_shape
        ])))
    else:
        shape_hash = shape.hash(feature_shape)

    return PRIVATE_DO_NOT_USE_feature_target_name(
        clean_target_name(
            "antlir_feature__{name}__KEY_{key}__{include_in_name}__{shape_hash}".format(
                name = name,
                key = key,
                include_in_name = include_in_name_str,
                shape_hash = shape_hash,
            ),
        ),
    )
