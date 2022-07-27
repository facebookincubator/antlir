# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/bzl/image/feature:new.bzl",
    "PRIVATE_DO_NOT_USE_feature_target_name",
)

def _clean_arg_to_str(arg):
    # chars that can be included in target name.
    valid_target_chars_str = ("ABCDEFGHIJKLMNOPQRSTUVWXYZ" +
                              "abcdefghijklmnopqrstuvwxyz" +
                              "0123456789" +
                              "_,.=-\\~@!+$")
    valid_target_chars = sets.make(
        [valid_target_chars_str[i] for i in range(len(valid_target_chars_str))],
    )

    # chars that can't be included in target name and should also be entirely
    # removed from `str(arg)` (replaced with ""). All characters not in
    # `remove_chars` and not in `valid_target_chars` are replaced with
    # underscores to improve readability.
    remove_chars_str = "][}{)(\"' "
    remove_chars = sets.make(
        [remove_chars_str[i] for i in range(len(remove_chars_str))],
    )

    arg_str = str(arg)
    return "".join(
        [
            arg_str[i] if sets.contains(valid_target_chars, arg_str[i]) else "_"
            for i in range(len(arg_str))
            if not sets.contains(remove_chars, arg_str[i])
        ],
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
                    arg = _clean_arg_to_str(arg),
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
        "antlir_feature__{name}__KEY_{key}__{include_in_name}__{shape_hash}".format(
            name = name,
            key = key,
            include_in_name = include_in_name_str,
            shape_hash = shape_hash,
        ),
    )

def _self_test_generate_feature_target_name():
    test_shape = shape.shape(
        str_f = shape.field(str, optional = True),
        int_f = shape.field(int, optional = True),
        dict_f = shape.field(shape.dict(str, str), optional = True),
    )

    test_shape_1 = test_shape(
        str_f = "foo",
        int_f = 12345,
    )
    test_shape_2 = test_shape(
        str_f = "bar",
        dict_f = {
            "bar": "baz",
            "foo": "bar",
        },
    )

    test_1 = generate_feature_target_name(
        name = "foo",
        key = "key1",
        feature_shape = test_shape_1,
        include_in_name = {
            "bar": ["baz"],
        },
    )
    test_2 = generate_feature_target_name(
        name = "bar",
        key = "key2",
        feature_shape = [test_shape_1, test_shape_2],
        include_in_name = {
            "bar": [],
        },
    )

    test_1_answer = (
        "antlir_feature__foo__KEY_key1__BAR_baz__" +
        "6ygUSY8247Ckf4dVALue5R3cAu90u6uXsBf4lSB-8Rc" +
        "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    )
    test_2_answer = (
        "antlir_feature__bar__KEY_key2____" +
        "QW5m92VBPiWeDkvqaMarAQTNOtYRGEcoEf6SrtBDB1U" +
        "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    )

    if test_1 != test_1_answer or test_2 != test_2_answer:
        fail(
            "{function} failed test".format(
                function = "generate_feature_target_name",
            ),
        )

_self_test_generate_feature_target_name()
