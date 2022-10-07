# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:flavor_impl.bzl", "flavors_to_structs")
load("//antlir/bzl/image/feature:rpm_install_info_dummy_action_item.bzl", "RPM_INSTALL_INFO_DUMMY_ACTION_ITEM")
load(":feature_info.bzl", "InlineFeatureInfo")

def rpms_install(rpmlist: [str.type], flavors: [[str.type], None] = None):
    return _build_rpm_rules("install", rpmlist, flavors)

def rpms_remove_if_exists(rpmlist: [str.type], flavors: [[str.type], None] = None):
    return _build_rpm_rules("remove_if_exists", rpmlist, flavors)

_flavor_to_version_set_prefix = "flavor_to_version_set:"
_action_enum = enum("install", "remove_if_exists")

def _rpms(
        action: _action_enum.type,
        rpm: str.type,
        flavor_to_version_set: {str.type: str.type},
        flavors_specified: bool.type):
    sources = {}
    if ":" in rpm:
        sources["source"] = rpm
    sources.update({
        "{}{}".format(_flavor_to_version_set_prefix, k): v
        for k, v in flavor_to_version_set.items()
        if v != BZL_CONST.version_set_allow_all_versions
    })

    return InlineFeatureInfo(
        feature_type = "rpm",
        sources = sources,
        kwargs = {
            "action": action,
            "flavors_specified": flavors_specified,
            "flavors_to_allow_all_versions": [k for k, v in flavor_to_version_set.items() if v == BZL_CONST.version_set_allow_all_versions],
            "rpm": rpm,
        },
        deps = {},
    )

def _build_rpm_rules(action, rpmlist, flavors):
    flavors = flavors_to_structs(flavors)
    flavors_specified = len(flavors) > 0
    rpms = []
    if action == "install":
        rpms.append(_rpms(
            rpm = RPM_INSTALL_INFO_DUMMY_ACTION_ITEM,
            action = "install",
            flavor_to_version_set = {flavor.name: BZL_CONST.version_set_allow_all_versions for flavor in flavors},
            flavors_specified = flavors_specified,
        ))

    needs_version_set = (action == "install")

    for name_or_source in rpmlist:
        vs_name = None
        if needs_version_set and ":" not in name_or_source:
            vs_name = name_or_source

        flavor_to_version_set = {}
        for flavor in flavors or flavors_to_structs(REPO_CFG.flavor_to_config.keys()):
            vs_path_prefix = REPO_CFG.flavor_to_config[flavor.name].version_set_path

            # We just add the version set for user given flavors, even
            # if they are invalid. They will be added as dependencies of the
            # image layer that uses this feature.
            if vs_path_prefix != BZL_CONST.version_set_allow_all_versions and vs_name:
                vs_target = vs_path_prefix + "/rpm:" + vs_name
                flavor_to_version_set[flavor.name] = vs_target
            else:
                flavor_to_version_set[flavor.name] = BZL_CONST.version_set_allow_all_versions
        rpms.append(
            _rpms(
                action = action,
                rpm = name_or_source,
                flavor_to_version_set = flavor_to_version_set,
                flavors_specified = flavors_specified,
            ),
        )

    return rpms

def rpms_to_json(
        action: _action_enum.type,
        rpm: str.type,
        flavors_specified: bool.type,
        flavors_to_allow_all_versions: [str.type],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    name = rpm
    source = sources.pop("source", None)
    if source:
        name = None

    flavor_to_version_set = {}
    for k, v in sources.items():
        if k.startswith(_flavor_to_version_set_prefix):
            flavor_to_version_set[k[len(_flavor_to_version_set_prefix):]] = v

    for k in flavors_to_allow_all_versions:
        flavor_to_version_set[k] = BZL_CONST.version_set_allow_all_versions

    return {
        "action": action,
        "flavor_to_version_set": flavor_to_version_set,
        "flavors_specified": flavors_specified,
        "name": name,
        "source": source,
    }
