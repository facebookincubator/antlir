# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")

def write_os(path: str):
    return feature.install_text(
        dst = path,
        text = select({
            "//antlir/antlir2/os:centos8": "centos8",
            "//antlir/antlir2/os:centos9": "centos9",
            "//antlir/antlir2/os:eln": "eln",
        }),
    )
