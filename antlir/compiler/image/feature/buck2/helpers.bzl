# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
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
        include_in_name = sorted(filter(
            lambda x: x[1],
            include_in_name.items(),
        ))

    include_in_name_str = "".join([
        "{arg_name}_{arg}_".format(
            arg_name = arg_name.upper(),
            arg = _clean_arg_to_str(arg),
        )
        for arg_name, arg in include_in_name
    ])[:-1] if include_in_name else ""

    shape_hash = sha256_b64(shape.do_not_cache_me_json(feature_shape))

    return PRIVATE_DO_NOT_USE_feature_target_name(
        "antlir_feature__{name}__KEY_{key}__{include_in_name}__{shape_hash}".format(
            name = name,
            key = key,
            include_in_name = include_in_name_str,
            shape_hash = shape_hash,
        ),
    )

def _self_test():
    test_shape = shape.shape(
        str_f = shape.field(str, optional = True),
        int_f = shape.field(int, optional = True),
        dict_f = shape.field(shape.dict(str, str), optional = True),
    )

    test_shape_1 = shape.new(
        test_shape,
        str_f = "foo",
        int_f = 12345,
    )
    test_shape_2 = shape.new(
        test_shape,
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
        feature_shape = test_shape_2,
        include_in_name = {
            "bar": [],
        },
    )

    test_1_answer = "antlir_feature__foo__KEY_key1__BAR_baz__bINw2KkLqCDPdHGk1Zq3xcFnfHpglxM_H4v2UxBoj80_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    test_2_answer = "antlir_feature__bar__KEY_key2____FlaeIVArr0LvDeMArSqZ3MVXyWViBveI7_Eh2auc2MY_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"

    if (
        test_1 != test_1_answer or
        test_2 != test_2_answer
    ):
        fail("{function} failed test".format(
            function = "generate_feature_target_name",
        ))

_self_test()
