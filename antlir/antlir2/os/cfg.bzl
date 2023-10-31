# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":defs.bzl", "OsVersionInfo")

_OS_REFS = {
    "os.centos8": "//antlir/antlir2/os:centos8",
    "os.centos9": "//antlir/antlir2/os:centos9",
    "os.eln": "//antlir/antlir2/os:eln",
    "os.none": "//antlir/antlir2/os:none",
    "os_constraint": "//antlir/antlir2/os:os",
    "os_family_constraint": "//antlir/antlir2/os/family:family",
}

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

def remove_os_constraints(*, constraints, refs):
    constraints.pop(refs.os_constraint[ConstraintSettingInfo].label, None)
    constraints.pop(refs.os_family_constraint[ConstraintSettingInfo].label, None)
    constraints.pop(refs.package_manager_constraint[ConstraintSettingInfo].label, None)
    return constraints
