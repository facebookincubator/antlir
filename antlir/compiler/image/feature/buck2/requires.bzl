# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:requires.shape.bzl", "requires_t")
load(":helpers.bzl", "generate_feature_target_name")
load(":providers.bzl", "feature_provider")

def feature_requires_rule_impl(ctx: "context") -> ["provider"]:
    return feature_provider(
        "requires",
        shape.new(
            requires_t,
            users = ctx.attr.users,
            groups = ctx.attr.groups,
            files = ctx.attr.files,
        ),
    )

feature_requires_rule = rule(
    implementation = feature_requires_rule_impl,
    attrs = {
        "files": attr.list(attr.string(), default = []),
        "groups": attr.list(attr.string(), default = []),
        "users": attr.list(attr.string(), default = []),
    },
)

def feature_requires(
        users = None,
        groups = None,
        files = None):
    """
`feature.requires(...)` adds macro-level requirements on image layers.

Currently this supports requiring users, groups and files to exist in the layer
being built. This feature doesn't materialize anything in the built image, but it
will cause a compiler error if any of the users/groups that are requested do not
exist in either the `parent_layer` or the layer being built.

An example of a reasonable use-case of this functionality is defining a macro
that generates systemd units that run as a specific user, where
`feature.requires` can be used for additional compile-time safety that the user,
groups or files do indeed exist.
"""
    target_name = generate_feature_target_name(
        name = "requires",
        include_in_name = {
            "files": files,
            "groups": groups,
            "users": users,
        },
    )

    if not native.rule_exists(target_name):
        feature_requires_rule(
            name = target_name,
            users = users,
            groups = groups,
            files = files,
        )

    return ":" + target_name

def _sha256_list(lst):
    return sha256_b64(str(sorted(lst or [])))
