# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")

mode_t = enum("none", "shadow")

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
