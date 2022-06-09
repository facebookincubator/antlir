# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:new.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")
load("//antlir/bzl/image/feature:remove.shape.bzl", "remove_paths_t")
load(":providers.bzl", "feature_provider")

def feature_remove_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        "remove_paths",
        shape.new(
            remove_paths_t,
            path = ctx.attr.dest,
            must_exist = ctx.attr.must_exist,
        ),
    )

feature_remove_rule = rule(
    implementation = feature_remove_rule_impl,
    attrs = {
        "dest": attr.string(),
        "must_exist": attr.bool(),
    },
)

def feature_remove(dest, must_exist = True):
    """
`feature.remove("/a/b")` recursively removes the file or directory `/a/b` --

These are allowed to remove paths inherited from the parent layer, or those
installed by RPMs even in this layer. However, removing other items
explicitly added by the current layer is not allowed since that seems like a
design smell -- you should probably refactor the constituent image features
not to conflict with each other.

By default, it is an error if the specified path is missing from the image,
though this can be avoided by setting `must_exist` to `False`.
    """
    target_name = PRIVATE_DO_NOT_USE_feature_target_name(dest + "__feature_remove")

    if not native.rule_exists(target_name):
        feature_remove_rule(
            name = target_name,
            dest = dest,
            must_exist = must_exist,
        )

    return ":" + target_name
