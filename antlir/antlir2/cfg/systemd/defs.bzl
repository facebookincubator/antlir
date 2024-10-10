# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _transition(*, constraints, refs: struct, attrs: struct, overwrite: bool):
    setting = refs.systemd_setting[ConstraintSettingInfo]
    if attrs.systemd and (
        (setting.label not in constraints) or
        overwrite
    ):
        if attrs.systemd == "cd":
            constraints[setting.label] = refs.systemd_cd[ConstraintValueInfo]
        elif attrs.systemd == "stable":
            systemd_stable = refs.systemd_stable[ConstraintValueInfo]
            constraints[setting.label] = systemd_stable
        elif attrs.systemd == "canary":
            systemd_canary = refs.systemd_canary[ConstraintValueInfo]
            constraints[setting.label] = systemd_canary
        elif attrs.systemd == "inherit-parent":
            pass
        else:
            fail("unknown systemd config '{}'".format(attrs.systemd))
    if not attrs.systemd and setting.label not in constraints:
        constraints[setting.label] = refs.systemd_stable[ConstraintValueInfo]
    if attrs.systemd == "inherit-parent":
        constraints.pop(setting.label, None)

    return constraints

systemd_cfg = struct(
    transition = _transition,
    refs = {
        "systemd_canary": "antlir//antlir/antlir2/cfg/systemd:systemd-canary",
        "systemd_cd": "antlir//antlir/antlir2/cfg/systemd:systemd-cd",
        "systemd_setting": "antlir//antlir/antlir2/cfg/systemd:systemd-setting",
        "systemd_stable": "antlir//antlir/antlir2/cfg/systemd:systemd-stable",
    },
    attrs = {
        "systemd": attrs.option(attrs.enum(["cd", "stable", "canary", "inherit-parent"]), default = None),
    },
)
