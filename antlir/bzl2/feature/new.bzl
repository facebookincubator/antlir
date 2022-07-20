# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load("//antlir/bzl2:flatten_features_list.bzl", "flatten_features_list")
load("//antlir/bzl2:is_build_appliance.bzl", "is_build_appliance")
load(
    "//antlir/bzl2:providers.bzl",
    "FeatureInfo",
    "FlavorInfo",
    "ItemInfo",
    "RpmInfo",
)

def _filter_rpm_versions(
        feature_dict,
        feature_flavors,
        is_layer_feature,
        from_feature_new = False):
    # The rpm item dicts are immutable, so copies need to be made to
    # allow them to be modified
    filtered_feature_dict = {}
    filtered_feature_dict["target"] = feature_dict["target"]
    filtered_feature_dict["rpms"] = [dict(r) for r in feature_dict["rpms"]]

    # Only include flavors which are in `feature_flavors` in
    # `flavor_to_version_set`
    for rpm_item in filtered_feature_dict["rpms"]:
        flavor_to_version_set = {}
        for flavor, version_set in rpm_item["flavor_to_version_set"].items():
            if not feature_flavors or flavor in feature_flavors:
                flavor_to_version_set[flavor] = version_set

        # Rpm item is required to share at least one flavor with new feature,
        # if it's from an `feature.rpms_install` target. If it's from a
        # `feature.new` target, it is allowed to be filtered out if it shares
        # no flavors
        if not from_feature_new and not flavor_to_version_set:
            fail(
                "Rpm `{rpm}` must have one of the flavors `{feature_flavors}`"
                    .format(
                    rpm = rpm_item["name"],
                    feature_flavors = feature_flavors,
                ),
            )

        # If this call to `feature.new` is for a new image layer, there should
        # only be rpms for a single flavor since the flavor is known from a
        # provider. This is to test that the flavor provider is working.
        if is_layer_feature and len(flavor_to_version_set) > 1:
            fail("Layer features must have rpms for no more than 1 flavor")

        rpm_item["flavor_to_version_set"] = flavor_to_version_set

    return filtered_feature_dict

def _feature_new_rule_impl(ctx: "context") -> ["provider"]:
    parent_layer_feature = ctx.attr.parent_layer_feature
    is_layer_feature = BZL_CONST.layer_feature_suffix in ctx.attr.name
    feature_flavors = ctx.attr.flavors

    if parent_layer_feature and feature_flavors:
        fail("`feature.new` can't be passed flavors from both `flavors` and " +
             "`parent_layer`")

    if (not feature_flavors and
        parent_layer_feature and
        parent_layer_feature[FlavorInfo]):
        feature_flavors = parent_layer_feature[FlavorInfo].flavors

    inline_features = []
    rpm_install_flavors = sets.make()
    for feature in ctx.attr.features:
        # If `feature[FeatureInfo]` exists, then `feature` was generated using
        # the `feature_new` macro and the `inline_features` of that feature are
        # appended onto this feature's `inline_features`.
        if feature[FeatureInfo]:
            for feature_dict in feature[FeatureInfo].inline_features:
                if "rpms" in feature_dict:
                    feature_dict = _filter_rpm_versions(
                        feature_dict,
                        feature_flavors,
                        is_layer_feature,
                        from_feature_new = True,
                    )
                inline_features.append(feature_dict)

        elif feature[ItemInfo]:
            feature_dict = structs.to_dict(feature[ItemInfo].items)
            feature_dict["target"] = ctx.attr.normalized_name

            if feature[RpmInfo]:
                if feature[RpmInfo].action == "rpms_install":
                    rpm_install_flavors = sets.union(
                        rpm_install_flavors,
                        sets.make(feature[RpmInfo].flavors),
                    )
                feature_dict = _filter_rpm_versions(
                    feature_dict,
                    feature_flavors,
                    is_layer_feature,
                )

            inline_features.append(feature_dict)

    # Skip coverage check for `antlir_test` since it's just for testing purposes
    # and doesn't always need to be covered.
    if feature_flavors:
        required_flavors = feature_flavors
    else:
        required_flavors = [
            flavor
            for flavor in REPO_CFG.stable_flavors
            if flavor != "antlir_test"
        ]
    required_flavors = sets.make(required_flavors)
    missing_flavors = sets.difference(required_flavors, rpm_install_flavors)

    # If installing rpms, at least one rpm must be installed for every flavor
    # passed to `feature_new`.
    if sets.length(rpm_install_flavors) and sets.length(missing_flavors):
        fail(("Missing `rpms_install` for flavors `{missing_flavors}`. " +
              "Expected `{required_flavors}`").format(
            missing_flavors = sets.to_list(missing_flavors),
            required_flavors = sets.to_list(required_flavors),
        ))

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
    ] + ([
        FlavorInfo(flavors = feature_flavors),
    ] if BZL_CONST.layer_feature_suffix in ctx.attr.name else [])

_feature_new_rule = rule(
    impl = _feature_new_rule_impl,
    attrs = {
        "features": attrs.list(attr.dep(), default = []),
        "flavors": attrs.list(attr.string(), default = []),
        "normalized_name": attrs.string(),

        # parent layer flavor can be fetched from parent layer feature
        "parent_layer_feature": attrs.option(attr.dep()),

        # for query (needed because `feature.new` can depend on targets that
        # need their on-disk location to be known)
        "type": attrs.string(default = "image_feature"),
    },
)

def feature_new(
        name,
        features,
        visibility = None,
        # This is used when a user wants to declare a feature
        # that is not available for all flavors in REPO_CFG.flavor_to_config.
        # An example of this is the internal feature in `image_layer.bzl`.
        flavors = None,
        # only used as argument in `compile_image_features.bzl`
        parent_layer = None):
    """
    Turns a group of image actions into a Buck target, so it can be
    referenced from outside the current project via `//path/to:name`.

    Do NOT use this for composition within one project, just use a list.

    See the file docblock for more details on image action composition.

    See other `.bzl` files in this directory for actions that actually build
    the container (install RPMs, remove files/directories, create symlinks
    or directories, copy executable or data files, declare mounts).
    """
    if (BZL_CONST.layer_feature_suffix in name and
        parent_layer and
        not is_build_appliance(parent_layer)):
        parent_layer_feature = parent_layer + BZL_CONST.layer_feature_suffix
    else:
        parent_layer_feature = None

    if not native.rule_exists(name):
        _feature_new_rule(
            name = name,
            normalized_name = normalize_target(":" + name),
            features = flatten_features_list(features),
            flavors = flavors,
            parent_layer_feature = parent_layer_feature,
            visibility = visibility,
        )
