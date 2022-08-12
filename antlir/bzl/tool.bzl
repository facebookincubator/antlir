# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":oss_shim.bzl", "config")

TOOLS = {
    target: target.replace("//", "/").replace(":", "/")
    for target in (
        config.get_antlir_cell_name() + "//antlir/debian:apt-proxy",
        config.get_antlir_cell_name() + "//antlir/bzl/shape2:ir2code",
    )
}

# this is still on dotslash until the end of this stack
TOOLS[config.get_antlir_cell_name() + "//antlir/bzl/shape2:bzl2ir.rc"] = (config.get_antlir_cell_name() + "//antlir/bzl/shape2:bzl2ir").replace("//", "/").replace(":", "/")
