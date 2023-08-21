# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", antlir2_feature = "feature")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

def _autologin(use_antlir2 = False):
    """
    Install image features to enable auto login of root on the ttyS0
    serial console.
    """
    return _antlir2() if use_antlir2 else _antlir1()

def _antlir2():
    return [
        antlir2_feature.install(
            src = "//antlir/linux/vm/console:autologin-root.conf",
            dst = "/usr/lib/systemd/system/serial-getty@ttyS0.service.d/autologin-root.conf",
        ),
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/usr/lib/systemd/system",
            subdirs_to_create = "serial-getty@ttyS0.service.d",
            mode = "a+rx,u+w",
        ),
    ]

def _antlir1():
    return [
        antlir1_feature.install(
            "//antlir/linux/vm/console:autologin-root.conf",
            "/usr/lib/systemd/system/serial-getty@ttyS0.service.d/autologin-root.conf",
        ),
        antlir1_feature.ensure_subdirs_exist(
            "/usr/lib/systemd/system",
            "serial-getty@ttyS0.service.d",
            mode = "a+rx,u+w",
        ),
    ]

console = struct(
    autologin = _autologin,
)
