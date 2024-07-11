# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "internal_external", "is_facebook")
load(":defs.bzl", "OsVersionInfo")

_OS_REFS = {
    "os.centos10": "antlir//antlir/antlir2/os:centos10",
    "os.centos8": "antlir//antlir/antlir2/os:centos8",
    "os.centos9": "antlir//antlir/antlir2/os:centos9",
    "os.eln": "antlir//antlir/antlir2/os:eln",
    "os.none": "antlir//antlir/antlir2/os:none",
    "os.rhel8": "antlir//antlir/antlir2/os:rhel8",
    "os.rhel8.8": "antlir//antlir/antlir2/os:rhel8.8",
    "os_constraint": "antlir//antlir/antlir2/os:os",
    "os_family_constraint": "antlir//antlir/antlir2/os/family:family",
    "package_manager_constraint": "antlir//antlir/antlir2/os/package_manager:package_manager",
} | internal_external(
    fb = {
        "rou_constraint": "antlir//antlir/antlir2/os/facebook:rou",  # @
    },
    oss = {},
)

def os_transition_refs():
    return _OS_REFS

def os_transition(
        *,
        default_os: str,
        constraints,
        refs: struct,
        overwrite: bool = False):
    os = getattr(refs, "os." + default_os)[OsVersionInfo]
    os_constraint = os.constraint[ConstraintValueInfo]
    family = os.family[ConstraintValueInfo]
    package_manager = os.package_manager[ConstraintValueInfo]

    if overwrite or os_constraint.setting.label not in constraints:
        constraints[os_constraint.setting.label] = os_constraint
        constraints[family.setting.label] = family
        constraints[package_manager.setting.label] = package_manager

    return constraints

def _remove_os_transition_impl(platform, refs):
    constraints = platform.configuration.constraints
    constraints.pop(refs.os_constraint[ConstraintSettingInfo].label, None)
    constraints.pop(refs.os_family_constraint[ConstraintSettingInfo].label, None)
    if is_facebook:
        constraints.pop(refs.rou_constraint[ConstraintSettingInfo].label, None)
    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

remove_os_transition = transition(
    impl = _remove_os_transition_impl,
    refs = _OS_REFS,
)
