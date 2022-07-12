# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _install():
    """
    Return features to install the necessary configuration to notify
    the `antlir.vm` runtime when a host has booted.
    """

    # The notify-host service is activated by a udev rule, ensuring that it only
    # activates after the virtserialport has been activated and symlinked in
    # /dev/virtio-ports.
    return [
        systemd.install_unit(
            "//antlir/linux/vm/notify:notify-host.service",
        ),
        feature.ensure_subdirs_exist(
            "/usr/lib/udev",
            "rules.d",
            0o755,
        ),
        feature.install(
            "//antlir/linux/vm/notify:notify-host.rules",
            "/usr/lib/udev/rules.d/99-notify-host.rules",
        ),
    ]

notify = struct(
    install = _install,
)
