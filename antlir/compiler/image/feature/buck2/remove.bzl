# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:remove.shape.bzl", "remove_paths_t")
load(":rules.bzl", "maybe_add_feature_rule")

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
    return maybe_add_feature_rule(
        name = "remove",
        key = "remove_paths",
        include_in_target_name = {"dest": dest},
        shape = shape.new(
            remove_paths_t,
            path = dest,
            must_exist = must_exist,
        ),
    )
