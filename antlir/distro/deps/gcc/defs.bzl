# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/distro/deps:defs.bzl", "format_select")

def gcc_path_select(path: str) -> Select:
    """
    Selects gcc paths which often have triples and version in them.
    """
    return format_select(
        path,
        triple = arch_select(
            aarch64 = "aarch64-redhat-linux",
            x86_64 = "x86_64-redhat-linux",
        ),
        version = select({
            "DEFAULT": "11",
            "antlir//antlir/antlir2/os:centos10": "14",
            "antlir//antlir/antlir2/os:centos9": "11",
        }),
    )
