# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")
load("//antlir/bzl:stat.bzl", "stat")

device_type = enum("block", "char")

def mknod(
        *,
        dst: str | Select,
        major: int | Select,
        minor: int | Select,
        type: device_type | Select,
        user: str | Select = "root",
        group: str | Select = "root",
        mode: int | str | Select = 0o600) -> ParseTimeFeature:
    """
    `mknod("/dev/console", 644, "c", "user", "group", 5, 1)` creates a character device file
    in /dev/console owned by user:group with mode 0644
    """

    return ParseTimeFeature(
        feature_type = "mknod",
        plugin = antlir2_dep("//antlir/antlir2/features/mknod:mknod"),
        kwargs = {
            "dst": dst,
            "group": group,
            "major": major,
            "minor": minor,
            "mode": stat.mode(mode),
            "type": type.value,
            "user": user,
        },
    )

mknod_rule = data_only_feature_rule(
    feature_type = "mknod",
    feature_attrs = {
        "build_phase": attrs.enum(BuildPhase.values(), default = "compile"),
        "dst": attrs.string(),
        "group": attrs.string(),
        "major": attrs.int(),
        "minor": attrs.int(),
        "mode": attrs.int(),
        "type": attrs.enum(["block", "char"]),
        "user": attrs.string(),
    },
)
