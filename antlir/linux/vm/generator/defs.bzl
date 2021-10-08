# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _mounts():
    """
    Install the `antlir.vm` mount generator for setting up 9p and other
    mounts needed for testing images in vms.
    """

    return [
        feature.install(
            "//antlir/vm:mount-generator",
            "/usr/lib/systemd/system-generators/mount-generator",
            mode = "a+rx",
        ),
    ]

generator = struct(
    mounts = _mounts,
)
