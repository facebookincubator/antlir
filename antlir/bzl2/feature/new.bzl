# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load(
    "//antlir/bzl/image/feature:new.bzl",
    "PRIVATE_DO_NOT_USE_feature_target_name",
)
load(
    "//antlir/bzl/image/feature:rpm_install_info_dummy_action_item.bzl",
    "RPM_INSTALL_INFO_DUMMY_ACTION_ITEM",
)
load("//antlir/bzl2:flatten_features_list.bzl", "flatten_features_list")
load("//antlir/bzl2:image_source_helper.bzl", "mark_path", "unwrap_path")
load("//antlir/bzl2:is_build_appliance.bzl", "is_build_appliance")
load(
    "//antlir/bzl2:providers.bzl",
    "FlavorInfo",
    "ItemInfo",
)
load("//antlir/bzl2:use_buck2_macros.bzl", "use_buck2_macros")

def _clean_items_and_validate_flavors(rpm_items, flavors):
    feature_dicts = []
    rpm_install_flavors = sets.make()
    for feature_dict in rpm_items:
        feature_dict = dict(feature_dict)
        valid_rpms = []
        for rpm_item in feature_dict.get("rpms", []):
            if rpm_item["action"] == "install":
                for flavor, _ in rpm_item["flavor_to_version_set"].items():
                    sets.insert(rpm_install_flavors, flavor)

            # We add a dummy in `_build_rpm_feature` in `rpms.bzl`
            # to hold information about the action and flavor for
            # empty rpm lists for validity checks.
            # See the comment in `_build_rpm_feature` for more
            # information.
            if rpm_item["name"] != RPM_INSTALL_INFO_DUMMY_ACTION_ITEM:
                valid_rpms.append(rpm_item)

        feature_dict["rpms"] = valid_rpms
        feature_dicts.append(feature_dict)

    # Skip coverage check for `antlir_test` since it's just for testing purposes
    # and doesn't always need to be covered.
    if flavors:
        required_flavors = flavors
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

    return feature_dicts

def _feature_new_rule_impl(ctx):
    parent_layer_feature = ctx.attrs.parent_layer_feature

    if ctx.attrs.flavors:
        flavors = ctx.attrs.flavors
    elif parent_layer_feature and parent_layer_feature[FlavorInfo]:
        flavors = parent_layer_feature[FlavorInfo].flavors
    else:
        flavors = []

    inline_features = []

    for i, feature in enumerate(ctx.attrs.features):
        if feature[ItemInfo]:
            feature_dict = structs.to_dict(feature[ItemInfo].items)
            feature_dict["target"] = ctx.attrs.normalized_name
        else:
            feature_dict = mark_path(ctx.attrs.normalized_features[i])

        inline_features.append(feature_dict)

    inline_features.extend(_clean_items_and_validate_flavors(ctx.attrs.rpm_items, flavors))

    items = struct(
        target = ctx.attrs.name,
        features = inline_features,
    )

    output = ctx.actions.declare_output("items.json")
    ctx.actions.write_json(output, items)

    return [
        native.DefaultInfo(default_outputs = [output]),
    ] + ([
        FlavorInfo(flavors = flavors),
    ] if BZL_CONST.layer_feature_suffix in ctx.attrs.name else [])

_feature_new_rule = native.rule(
    impl = _feature_new_rule_impl,
    attrs = {
        "deps": native.attrs.list(native.attrs.dep(), default = []),
        "features": native.attrs.list(native.attrs.dep(), default = []),
        "flavors": native.attrs.list(native.attrs.string(), default = []),
        "normalized_features": native.attrs.list(native.attrs.string(), default = []),
        "normalized_name": native.attrs.string(),

        # parent layer flavor can be fetched from parent layer feature
        "parent_layer_feature": native.attrs.option(native.attrs.dep()),
        "rpm_items": native.attrs.list(native.attrs.dict(native.attrs.string(), native.attrs.any())),

        # for query (needed because `feature.new` can depend on targets that
        # need their on-disk location to be known)
        "type": native.attrs.string(default = "image_feature"),
    },
) if use_buck2_macros() else None

def normalize_features(
        target_name,
        targets_or_rpm_structs,
        flavors):
    targets = []
    rpm_items = []
    rpm_deps = []
    for item in flatten_features_list(targets_or_rpm_structs):
        if types.is_string(item):
            targets.append(PRIVATE_DO_NOT_USE_feature_target_name(
                normalize_target(item),
            ) if not item.endswith(BZL_CONST.PRIVATE_feature_suffix) else normalize_target(item))
        else:
            feature_dict = {
                "rpms": [
                    shape.as_serializable_dict(rpm_item)
                    for rpm_item in item.rpm_items
                ],
                "target": normalize_target(":" + target_name),
            }

            rpm_item_deps = {dep: 1 for dep in item.rpm_deps}
            for rpm_item in feature_dict.get("rpms", []):
                flavor_to_version_set = {}
                for flavor, version_set in rpm_item.get("flavor_to_version_set", {}).items():
                    # If flavors are not provided, we are reading the flavor
                    # from the parent layer, so we should include all possible flavors
                    # for the rpm as the final flavor is not known until we are in python.
                    if (
                        not flavors and (
                            rpm_item.get("flavors_specified") or
                            flavor in REPO_CFG.stable_flavors
                        )
                    ) or (
                        flavors and flavor in flavors
                    ):
                        flavor_to_version_set[flavor] = version_set
                    elif version_set != BZL_CONST.version_set_allow_all_versions:
                        target = unwrap_path(version_set)
                        rpm_item_deps.pop(target)

                if not flavor_to_version_set and rpm_item["name"] != RPM_INSTALL_INFO_DUMMY_ACTION_ITEM:
                    fail("Rpm `{}` must have one of the flavors `{}`".format(
                        rpm_item["name"] or rpm_item["source"],
                        flavors,
                    ))
                rpm_item["flavor_to_version_set"] = flavor_to_version_set

            rpm_deps.extend(rpm_item_deps.keys())
            rpm_items.append(feature_dict)

    return struct(
        targets = targets,
        rpm_items = rpm_items,
        rpm_deps = rpm_deps,
    )

def private_feature_new(
        name,
        features,
        visibility = None,
        flavors = None,
        parent_layer = None,
        deps = None):
    """
    `parent_layer` and `deps` are only used in `compile_image_features`.
    `parent_layer` is used to depend on the parent layer's layer feature and
    `deps` is for any other dependencies that this feature has that aren't
    features.
    """
    deps = deps or []
    flavors = flavors or []

    if (BZL_CONST.layer_feature_suffix in name and parent_layer and not is_build_appliance(parent_layer)):
        parent_layer_feature = PRIVATE_DO_NOT_USE_feature_target_name(parent_layer + BZL_CONST.layer_feature_suffix)
    else:
        parent_layer_feature = None

    target_name = PRIVATE_DO_NOT_USE_feature_target_name(name)

    if structs.is_struct(features):
        normalized_features = features
    else:
        normalized_features = normalize_features(name, features, flavors)

    if not native.rule_exists(name):
        _feature_new_rule(
            name = target_name,
            normalized_name = normalize_target(":" + name),
            features = normalized_features.targets,
            rpm_items = normalized_features.rpm_items,
            normalized_features = normalized_features.targets,
            flavors = flavors,
            parent_layer_feature = parent_layer_feature,
            deps = deps + normalized_features.rpm_deps,
            visibility = visibility,
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
    return private_feature_new(
        name,
        features,
        visibility,
        flavors,
    )
