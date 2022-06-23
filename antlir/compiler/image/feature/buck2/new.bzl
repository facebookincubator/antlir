# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:structs.bzl", "structs")
load(
    "//antlir/compiler/image/feature/buck2:providers.bzl",
    "FeatureInfo",
    "ItemInfo",
)

def _feature_new_rule_impl(ctx: "context") -> ["provider"]:
    inline_features = []
    for feature in ctx.attr.features:
        # If `feature[FeatureInfo]` exists, then `feature` was generated using
        # the `feature_new` macro and the `inline_features` of that feature are
        # appended onto this feature's `inline_features`.
        if feature[FeatureInfo]:
            inline_features += feature[FeatureInfo].inline_features
        else:
            feature_dict = structs.to_dict(feature[ItemInfo].items)
            feature_dict["target"] = ctx.attr.name
            inline_features.append(feature_dict)

    items = struct(
        target = ctx.attr.name,
        features = inline_features,
    )

    output = ctx.actions.declare_output("items.json")
    ctx.actions.write_json(output, items)

    return [
        DefaultInfo(default_outputs = [output]),
        FeatureInfo(
            inline_features = inline_features,
        ),
    ]

_feature_new_rule = rule(
    implementation = _feature_new_rule_impl,
    attrs = {
        "features": attr.list(attr.dep(), default = []),
    },
)

def feature_new(
        name,
        features,
        visibility = None,
        # This is used when a user wants to declare a feature
        # that is not available for all flavors in REPO_CFG.flavor_to_config.
        # An example of this is the internal feature in `image_layer.bzl`.
        flavors = None):
    """
    Turns a group of image actions into a Buck target, so it can be
    referenced from outside the current project via `//path/to:name`.

    Do NOT use this for composition within one project, just use a list.

    See the file docblock for more details on image action composition.

    See other `.bzl` files in this directory for actions that actually build
    the container (install RPMs, remove files/directories, create symlinks
    or directories, copy executable or data files, declare mounts).
    """
    if not native.rule_exists(name):
        _feature_new_rule(
            name = name,
            features = features,
        )
