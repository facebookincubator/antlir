# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/image/feature:requires.shape.bzl", "requires_t")
load(":rules.bzl", "maybe_add_feature_rule")

def feature_requires(users = None, groups = None, files = None):
    """
    `feature.requires(...)` adds macro-level requirements on image layers.

    Currently this supports requiring users, groups and files to exist in the
    layer being built. This feature doesn't materialize anything in the built
    image, but it will cause a compiler error if any of the users/groups that
    are requested do not exist in either the `parent_layer` or the layer being
    built.

    An example of a reasonable use-case of this functionality is defining a
    macro that generates systemd units that run as a specific user, where
    `feature.requires` can be used for additional compile-time safety that the
    user, groups or files do indeed exist."""
    return maybe_add_feature_rule(
        name = "requires",
        include_in_target_name = {
            "files": files,
            "groups": groups,
            "users": users,
        },
        feature_shape = requires_t(
            users = users,
            groups = groups,
            files = files,
        ),
    )
