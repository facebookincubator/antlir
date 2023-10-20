# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")

mode_t = enum(
    # no antlir2 shadowing by default
    "none",
    # shadow all layers, features, packages and tests with antlir2 definitions by default
    "shadow",
    # transparently upgrade all targets to antlir2.
    # antlir1 feature rules are kept around since they don't have conflicting
    # names with the porcelain antlir2 targets
    "upgrade",
)

def _configure_package(*, mode: str | mode_t):
    if types.is_string(mode):
        mode = mode_t(mode)
    write_package_value(
        "antlir2.migration",
        mode,
        overwrite = True,
    )

def _get_mode() -> mode_t:
    return read_package_value("antlir2.migration") or mode_t("none")

antlir2_migration = struct(
    configure_package = _configure_package,
    get_mode = _get_mode,
    mode_t = mode_t,
)
