# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/distro/deps:rpm_library.bzl", "rpm_library")

def boost_rpm_library(name, **kwargs) -> None:
    rpm_library(
        name = name,
        rpm = select({
            "//antlir/antlir2/os:centos10": ["boost-devel"],
            "//antlir/antlir2/os:centos9": ["boost1.78-devel"],
            "DEFAULT": ["boost-devel"],
        }),
        **kwargs
    )
