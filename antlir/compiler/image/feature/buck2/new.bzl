# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "BZL_CONST")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl/image/feature:new.bzl", "PRIVATE_DO_NOT_USE_feature_target_name")
load(":providers.bzl", "FeatureInfo", "ItemInfo")

def feature_new_rule_impl(ctx: "context") -> ["provider"]:
    inline_features = []
    for feature in ctx.attr.features:
        # If `feature[FeatureInfo]` exists, then `feature` was generated using the
        # `feature_new` macro and the `inline_features` of that feature are appended
        # onto this feature's `inline_features`.
        if feature[FeatureInfo]:
            inline_features += feature[FeatureInfo].inline_features
        else:
            feature_dict = {
                feature_key: [
                    (
                        shape.as_serializable_dict(f) if
                        # Some features have been converted to `shape`.  To make
                        # them serializable together with legacy features, we must
                        # turn these shapes into JSON-ready dicts.
                        #
                        # Specifically, this transformation removes the private
                        # `__shape__` field, and asserts that there are no
                        # `shape.target()` fields -- shapes are not integrated with
                        # target_tagger yet, so one has to explicitly target-tag the
                        # stuff that goes into these shapes.
                        #
                        # Future: once we shapify all features, this explicit
                        # conversion can be removed since shape serialization will
                        # just do the right thing.
                        shape.is_any_instance(f) else f
                    )
                    for f in features
                ]
                for feature_key, features in structs.to_dict(feature[ItemInfo].items).items()
            }
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

feature_new_rule = rule(
    implementation = feature_new_rule_impl,
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
    target_name = PRIVATE_DO_NOT_USE_feature_target_name(name)

    # Private suffixes need to be added to all feature targets that don't
    # already have a private suffix because targets created by calls to
    # `feature_new` have a private suffix, but we don't want to have to
    # include that suffix when declaring features inline.
    features = [
        feature if feature.endswith(BZL_CONST.PRIVATE_feature_suffix) else PRIVATE_DO_NOT_USE_feature_target_name(feature)
        for feature in features
    ]

    if not native.rule_exists(target_name):
        feature_new_rule(
            name = target_name,
            features = features,
        )
