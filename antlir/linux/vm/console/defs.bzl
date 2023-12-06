# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

TTY_NAME = arch_select(aarch64 = "ttyAMA0", x86_64 = "ttyS0")

def _autologin(use_antlir2 = False):
    """
    Install image features to enable auto login of root on the ttyS0
    serial console.
    """
    return _antlir2() if use_antlir2 else _antlir1()

def _antlir2():
    def autologin_features(tty):
        return [
            antlir2_feature.install(
                src = "//antlir/linux/vm/console:autologin-root.conf",
                dst = "/usr/lib/systemd/system/serial-getty@{}.service.d/autologin-root.conf".format(tty),
            ),
            antlir2_feature.ensure_subdirs_exist(
                into_dir = "/usr/lib/systemd/system",
                subdirs_to_create = "serial-getty@{}.service.d".format(tty),
                mode = "a+rx,u+w",
            ),
        ]

    # Let's spam on all potential console names as we are only adding a drop-in.
    return autologin_features("ttyS0") + autologin_features("ttyAMA0")

def _antlir1():
    # antlir1 only supports x86, so no need to switch names.
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
