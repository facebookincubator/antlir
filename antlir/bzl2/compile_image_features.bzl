# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/bzl:compile_image_features.bzl",
    "check_flavor",
    "compile_image_features_output",
    "vset_override_genrule",
)
load("//antlir/bzl:constants.bzl", "BZL_CONST")
load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl:query.bzl", "layer_deps_query", "query")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/bzl/image/feature:new.bzl",
    "PRIVATE_DO_NOT_USE_feature_target_name",
)
load("//antlir/bzl2/feature:new.bzl", "feature_new_internal")
load(":feature_rule.bzl", "maybe_add_feature_rule")
load(":flatten_features_list.bzl", "flatten_features_list")
load(":image_source_helper.bzl", "mark_path")
load(":is_build_appliance.bzl", "is_build_appliance")

def compile_image_features(
        name,
        current_target,
        parent_layer,
        features,
        flavor,
        flavor_config_override,
        extra_deps = None,
        subvol_name = None,
        internal_only_is_genrule_layer = False):
    '''
    Arguments

    - `subvol_name`: Future: eliminate this argument so that the build-time
    hardcodes this to "volume". Move this setting into btrfs-specific
    `package.new` options. See this post for more details
    https://fburl.com/diff/3050aw26
    '''
    if features == None:
        features = []
    if extra_deps == None:
        extra_deps = []

    check_flavor(
        flavor,
        parent_layer,
        flavor_config_override,
        name,
        current_target,
    )

    flavor_config = flavor_helpers.get_flavor_config(flavor, flavor_config_override) if flavor else None

    if flavor_config and flavor_config.build_appliance:
        extra_deps.append(flavor_config.build_appliance)

    # This is the list of supported flavors for the features of the layer.
    # A value of `None` specifies that no flavor field was provided for the layer.
    flavors = [flavor] if flavor else None

    if not flavors and is_build_appliance(parent_layer):
        flavors = [flavor_helpers.get_flavor_from_build_appliance(parent_layer)]

    if parent_layer:
        features.append(maybe_add_feature_rule(
            name = "parent_layer",
            include_in_target_name = {"parent_layer": parent_layer},
            feature_shape = shape.new(
                shape.shape(
                    subvol = shape.field(shape.dict(str, str)),
                ),
                subvol = mark_path(parent_layer, is_layer = True),
            ),
            deps = [parent_layer],
        ))

    features = [
        PRIVATE_DO_NOT_USE_feature_target_name(feature) if not feature.endswith(BZL_CONST.PRIVATE_feature_suffix) else feature
        for feature in flatten_features_list(features)
    ]

    # Outputs the feature JSON for the given layer to disk so that it can be
    # parsed by other tooling.
    #
    # Keep in sync with `bzl_const.py`.
    features_for_layer = name + BZL_CONST.layer_feature_suffix
    feature_new_internal(
        name = features_for_layer,
        features = features,
        flavors = flavors,
        parent_layer = parent_layer,
        deps = extra_deps,
        visibility = ["//antlir/..."],
    )
    features_for_layer = PRIVATE_DO_NOT_USE_feature_target_name(name + BZL_CONST.layer_feature_suffix)

    vset_override_name = vset_override_genrule(flavor_config, current_target)

    deps_query = query.union(
        [
            # We will query the deps of the features that are targets.
            query.deps(
                expr = query.attrfilter(
                    label = "type",
                    value = "image_feature",
                    expr = query.deps(
                        expr = query.set(features + extra_deps + [":" + features_for_layer]),
                        depth = query.UNBOUNDED,
                    ),
                ),
                depth = 1,
            ),
        ] + ([
            layer_deps_query(parent_layer),
        ] if parent_layer else []),
    )

    quoted_child_feature_json_args = (
        "--child-feature-json $(location {})".format(
            ":" + features_for_layer,
        )
    )

    return compile_image_features_output(
        name,
        current_target,
        parent_layer,
        flavor,
        flavor_config,
        subvol_name,
        internal_only_is_genrule_layer,
        vset_override_name,
        deps_query,
        quoted_child_feature_json_args,
    )
