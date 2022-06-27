# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:structs.bzl", "structs")
load(
    "//antlir/compiler/image/feature/buck2:providers.bzl",
    "FeatureInfo",
    "ItemInfo",
    "RpmInfo",
)

def _feature_new_rule_impl(ctx: "context") -> ["provider"]:
    feature_flavors = ctx.attr.flavors

    inline_features = []
    rpm_install_flavors = sets.make()
    for feature in ctx.attr.features:
        # If `feature[FeatureInfo]` exists, then `feature` was generated using
        # the `feature_new` macro and the `inline_features` of that feature are
        # appended onto this feature's `inline_features`.
        if feature[FeatureInfo]:
            inline_features += feature[FeatureInfo].inline_features
        elif feature[ItemInfo]:
            feature_dict = structs.to_dict(feature[ItemInfo].items)
            feature_dict["target"] = ctx.attr.name
            inline_features.append(feature_dict)
            if feature[RpmInfo]:
                if feature[RpmInfo].action == "rpms_install":
                    rpm_install_flavors = sets.union(
                        rpm_install_flavors,
                        sets.make(feature[RpmInfo].flavors),
                    )

                # The rpm item dicts are immutable, so copies need to be made to
                # allow them to be modified
                aliased_rpms = feature_dict["rpms"]
                feature_dict["rpms"] = [dict(r) for r in aliased_rpms]

                # Only include flavors which are in `feature_flavors` in
                # `flavor_to_version_set`
                for rpm_item in feature_dict["rpms"]:
                    flavor_to_version_set = {}
                    for flavor, version_set in rpm_item["flavor_to_version_set"].items():
                        if not feature_flavors or flavor in feature_flavors:
                            flavor_to_version_set[flavor] = version_set

                    # rpm item is required to share at least one flavor with
                    # new feature
                    if not flavor_to_version_set:
                        fail(("Rpm `{rpm}` must have one of the flavors " +
                              "`{feature_flavors}`").format(
                            rpm = rpm_item["name"],
                            feature_flavors = feature_flavors,
                        ))
                    rpm_item["flavor_to_version_set"] = flavor_to_version_set

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
    ]

_feature_new_rule = rule(
    implementation = _feature_new_rule_impl,
    attrs = {
        "features": attr.list(attr.dep(), default = []),
        "flavors": attr.list(attr.string(), default = []),
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
            flavors = flavors,
        )
