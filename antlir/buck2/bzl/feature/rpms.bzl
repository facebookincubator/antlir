# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:flavor.bzl", "coerce_to_flavor_label")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "InlineFeatureInfo")

def rpms_install(*, rpms: [str.type]):
    return _build_rpm_rules("install", rpms)

def rpms_remove_if_exists(*, rpms: [str.type]):
    return _build_rpm_rules("remove_if_exists", rpms)

_flavor_to_version_set_prefix = "flavor_to_version_set:"
_action_enum = enum("install", "remove_if_exists")
types.lint_noop(_action_enum)

def _rpms(
        action: _action_enum.type,
        rpm: str.type,
        flavor_to_version_set: {str.type: str.type}):
    sources = {}
    if ":" in rpm:
        sources["source"] = rpm

    flavor_specific_sources = {}
    for flavor, version_set in flavor_to_version_set.items():
        if flavor not in flavor_specific_sources:
            flavor_specific_sources[flavor] = {}
        flavor_specific_sources[flavor].update({
            "{}{}".format(_flavor_to_version_set_prefix, flavor): version_set,
        })

    return InlineFeatureInfo(
        feature_type = "rpm",
        sources = sources,
        flavor_specific_sources = flavor_specific_sources,
        kwargs = {
            "action": action,
            "rpm": rpm,
        },
    )

def _build_rpm_rules(action, rpmlist):
    rpms = []
    needs_version_set = (action == "install")

    for name_or_source in rpmlist:
        vs_name = None
        if needs_version_set and ":" not in name_or_source:
            vs_name = name_or_source

        flavor_to_version_set = {}
        for flavor in REPO_CFG.flavor_available:
            vs_path_prefix = REPO_CFG.flavor_to_config[flavor].version_set_path
            flavor = coerce_to_flavor_label(flavor)

            # We just add the version set for user given flavors, even
            # if they are invalid. They will be added as dependencies of the
            # image layer that uses this feature.
            if vs_path_prefix != BZL_CONST.version_set_allow_all_versions and vs_name and not vs_name.startswith("rpm-test-"):
                vs_target = vs_path_prefix + "/rpm:" + vs_name
                flavor_to_version_set[flavor] = vs_target

        rpms.append(
            _rpms(
                action = action,
                rpm = name_or_source,
                flavor_to_version_set = flavor_to_version_set,
            ),
        )

    return rpms

def rpms_to_json(
        action: _action_enum.type,
        rpm: str.type,
        sources: {str.type: "artifact"}) -> {str.type: ""}:
    name = rpm
    source = sources.pop("source", None)
    if source:
        name = None

    flavor_to_version_set = {}
    for k, v in sources.items():
        if k.startswith(_flavor_to_version_set_prefix):
            flavor_to_version_set[k[len(_flavor_to_version_set_prefix):]] = v

    if source:
        source = {"source": source}
    else:
        source = {"name": name}
    return {
        "action": action,
        "flavor_to_version_set": flavor_to_version_set,
        "source": source,
    }
