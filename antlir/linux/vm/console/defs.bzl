# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

TTY_NAME = arch_select(aarch64 = "ttyAMA0", x86_64 = "ttyS0")

def _autologin(tty = TTY_NAME, use_antlir2 = False):
    """
    Install image features to enable auto login of root on the ttyS0
    serial console.
    """
    return _antlir2(tty) if use_antlir2 else _antlir1(tty)

def _antlir2(tty):
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

def _antlir1(tty):
    return [
        antlir1_feature.install(
            "//antlir/linux/vm/console:autologin-root.conf",
            "/usr/lib/systemd/system/serial-getty@{}.service.d/autologin-root.conf".format(tty),
        ),
        antlir1_feature.ensure_subdirs_exist(
            "/usr/lib/systemd/system",
            "serial-getty@{}.service.d".format(tty),
            mode = "a+rx,u+w",
        ),
    ]

console = struct(
    autologin = _autologin,
)
