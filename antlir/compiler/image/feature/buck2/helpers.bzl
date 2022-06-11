# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@fbsource//tools/build_defs:type_defs.bzl", "type_utils")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl/image/feature:new.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")

def _clean_arg_to_str(arg):
    valid_target_chars = sets.make(("ABCDEFGHIJKLMNOPQRSTUVWXYZ" +
                                    "abcdefghijklmnopqrstuvwxyz" +
                                    "0123456789" +
                                    "_,.=-\\/~@!+$").split(""))

    return "".join([
        char
        for char in str(arg).split("")
        if sets.contains(valid_target_chars, char)
    ])

def generate_feature_target_name(name, include_in_name = None, include_only_in_hash = None):
    """
    Generate a memoization-safe target name.

    - `include_in_name`: feature arguments that are wanted in their original
    form (perhaps for debugging purposes).

    - `include_only_in_hash`: feature arguments that aren't needed in their
    original form, to be combined into a single hash along with those in
    `include_in_name`.
    """

    if include_in_name:
        include_in_name = list(filter(
            lambda x: x[1] not in (None, [], ""),
            sorted(include_in_name.items()),
        ))

    if include_only_in_hash:
        include_only_in_hash = sorted(include_only_in_hash.items())

    include_in_name_str = "".join([
        "{arg_name}_{arg}_".format(
            arg_name = arg_name.upper(),
            arg = _clean_arg_to_str(arg),
        )
        for arg_name, arg in include_in_name
    ])[:-1] if include_in_name else ""

    include_only_in_hash_str = sha256_b64(repr((include_in_name, include_only_in_hash)))

    return PRIVATE_DO_NOT_USE_feature_target_name(
        "antlir_feature__{name}__{include_in_name}__{include_only_in_hash}".format(
            name = name,
            include_in_name = include_in_name_str,
            include_only_in_hash = include_only_in_hash_str,
        ),
    )

def recursive_as_serializable_dict(val):
    """
    When `shape.as_serializable_dict` is used, nested shapes are converted to
    structs instead of dicts, which causes issues when passing to the
    `feature_shape` argument of feature_rule.
    """

    if type_utils.is_struct(val):
        val = shape.as_serializable_dict(val) if shape.is_any_instance(val) else structs.to_dict(val)

    elif type_utils.is_list(val) or type_utils.is_tuple(val):
        val = [recursive_as_serializable_dict(item) for item in val]
        if type_utils.is_tuple(val):
            val = tuple(val)

    if type_utils.is_dict(val):
        val = {k: recursive_as_serializable_dict(v) for k, v in val.items()}

    return val
