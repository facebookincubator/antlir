# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _host():
    """
    Configure the Guest -> Host networking inside the guest vm.
    """
    return [
        feature.install("//antlir/linux/vm/network:eth0.network", "/usr/lib/systemd/network/10-eth0.network"),
        feature.install("//antlir/linux/vm/network:eth0.link", "/usr/lib/systemd/network/10-eth0.link"),
        # make networkd require udevd so that eth0 can move past the "link pending udev initialization" state
        systemd.install_dropin("//antlir/linux/vm/network:require-udevd.conf", "systemd-networkd.service"),
        # empty resolv.conf since the only mechanism to refer to the host (by name) is via /etc/hosts
        "//antlir/linux/vm/network:resolvconf",
        feature.remove("/etc/hosts", must_exist = False),
        feature.install("//antlir/linux/vm/network:etc-hosts", "/etc/hosts"),
    ]

network = struct(
    host = _host,
)
