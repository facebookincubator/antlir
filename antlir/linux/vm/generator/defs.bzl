# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

def _mounts(use_antlir2 = False):
    """
    Install the `antlir.vm` mount generator for setting up 9p and other
    mounts needed for testing images in vms.
    """
    return _antlir2() if use_antlir2 else _antlir1()

def _antlir2():
    return [
        antlir2_feature.install(
            src = "//antlir/vm:mount-generator",
            dst = "/usr/lib/systemd/system-generators/mount-generator",
            mode = "a+rx",
        ),
    ]

def _antlir1():
    return [
        antlir1_feature.install(
            "//antlir/vm:mount-generator",
            "/usr/lib/systemd/system-generators/mount-generator",
            mode = "a+rx",
        ),
    ]

generator = struct(
    mounts = _mounts,
)
