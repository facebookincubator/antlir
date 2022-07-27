# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

load("//antlir/bzl:oss_shim.bzl", "is_buck2")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl2:generate_feature_target_name.bzl", "generate_feature_target_name")
load("//antlir/bzl2:providers.bzl", "ItemInfo")

def _feature_rule_impl(ctx):
    return [
        native.DefaultInfo(),
        ItemInfo(items = struct(**{ctx.attrs.key: [ctx.attrs.shape]})),
    ]

_feature_rule = native.rule(
    impl = _feature_rule_impl,
    attrs = {
        "deps": native.attrs.list(native.attrs.dep(), default = []),

        # corresponds to keys in `ItemFactory` in items_for_features.py
        "key": native.attrs.string(),

        # gets serialized to json when `feature.new` is called and used as
        # kwargs in compiler `ItemFactory`
        "shape": native.attrs.dict(native.attrs.string(), native.attrs.any()),

        # for query
        "type": native.attrs.string(default = "image_feature"),
    },
) if is_buck2() else None

def maybe_add_feature_rule(
        name,
        feature_shape,
        include_in_target_name = None,
        key = None,
        deps = []):
    # if `key` is not provided, then it is assumed that `key` is same as `name`
    key = key or name

    target_name = generate_feature_target_name(
        name = name,
        key = key,
        feature_shape = feature_shape,
        include_in_name = include_in_target_name,
    )

    if not native.rule_exists(target_name):
        _feature_rule(
            name = target_name,
            key = key,
            shape = shape.as_serializable_dict(
                feature_shape,
            ) if shape.is_any_instance(feature_shape) else feature_shape,
            deps = deps,
        )

    return ":" + target_name
