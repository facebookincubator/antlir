# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:systemd.bzl", "systemd")

def _install(use_antlir2 = False):
    """
    Return features to install the necessary configuration to notify
    the `antlir.vm` runtime when a host has booted.
    """

    return [
        systemd.install_unit(
            "//antlir/linux/vm/notify:virtio-notify@.service",
            use_antlir2 = use_antlir2,
        ),
        # Enable using the virtio socket named "notify-host"
        systemd.enable_unit(
            "virtio-notify@notify-host.service",
            use_antlir2 = use_antlir2,
        ),
    ]

notify = struct(
    install = _install,
)
