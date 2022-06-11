# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":helpers.bzl", "recursive_as_serializable_dict")
load(":providers.bzl", "feature_provider")

def feature_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        ctx.attr.key,
        ctx.attr.shape,
    )

feature_rule = rule(
    implementation = feature_rule_impl,
    attrs = {
        "deps": attr.list(attr.dep(), default = []),

        # corresponds to keys in `ItemFactory` in items_for_features.py
        "key": attr.string(),

        # gets serialized to json when `feature.new` is called and used as
        # kwargs in compiler `ItemFactor`
        "shape": attr.dict(attr.string(), attr.any()),

        # for query
        "type": attr.string(default = "image_feature"),
    },
)

def maybe_add_feature_rule(target_name, key, shape, deps = []):
    # TODO: generate target name from shape in this function
    if not native.rule_exists(target_name):
        feature_rule(
            name = target_name,
            key = key,
            shape = recursive_as_serializable_dict(shape),
            deps = deps,
        )

    return ":" + target_name
