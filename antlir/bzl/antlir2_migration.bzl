# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
load("//antlir/bzl:types.bzl?v2_only", "types")
load("//antlir/bzl:build_defs.bzl", "is_buck2") # @oss-enable

def _mode_t_fake_enum(x):
    return x

mode_t = native.enum(
    # no antlir2 shadowing by default
    "none",
    # shadow all layers, features, packages and tests with antlir2 definitions by default
    "shadow",
    # transparently upgrade all targets to antlir2.
    # antlir1 feature rules are kept around since they don't have conflicting
    # names with the porcelain antlir2 targets
    "upgrade",
) if is_buck2() else _mode_t_fake_enum

def _configure_package(*, mode: str | mode_t):
    if not is_buck2():
        return
    if types.is_string(mode):
        mode = mode_t(mode)

    # @lint-ignore-every BUCKLINT: avoid "native is forbidden in fbcode"
    native.write_package_value(
        "antlir2.migration",
        mode,
        overwrite = True,
    )

def _get_mode() -> mode_t:
    if not is_buck2():
        return mode_t("none")

    # @lint-ignore-every BUCKLINT: avoid "native is forbidden in fbcode"
    return native.read_package_value("antlir2.migration") or mode_t("none")

antlir2_migration = struct(
    configure_package = _configure_package,
    get_mode = _get_mode,
    mode_t = mode_t,
)
