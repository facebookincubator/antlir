# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

def _host(use_antlir2 = False):
    """
    Configure the Guest -> Host networking inside the guest vm.
    """
    if use_antlir2:
        return [
            antlir2_feature.install(
                src = "//antlir/linux/vm/network:eth0.network",
                dst = "/usr/lib/systemd/network/10-eth0.network",
            ),
            antlir2_feature.install(
                src = "//antlir/linux/vm/network:eth0.link",
                dst = "/usr/lib/systemd/network/10-eth0.link",
            ),
            # make networkd require udevd so that eth0 can move past the "link pending udev initialization" state
            systemd.install_dropin("//antlir/linux/vm/network:require-udevd.conf", "systemd-networkd.service", use_antlir2 = True),
            # empty resolv.conf since the only mechanism to refer to the host (by name) is via /etc/hosts
            "//antlir/linux/vm/network:resolvconf",
            antlir2_feature.remove(
                path = "/etc/hosts",
                must_exist = False,
            ),
            antlir2_feature.install(
                src = "//antlir/linux/vm/network:etc-hosts",
                dst = "/etc/hosts",
            ),
        ]

    # the rest of this function os for Antlir1
    return [
        antlir1_feature.install("//antlir/linux/vm/network:eth0.network", "/usr/lib/systemd/network/10-eth0.network"),
        antlir1_feature.install("//antlir/linux/vm/network:eth0.link", "/usr/lib/systemd/network/10-eth0.link"),
        # make networkd require udevd so that eth0 can move past the "link pending udev initialization" state
        systemd.install_dropin("//antlir/linux/vm/network:require-udevd.conf", "systemd-networkd.service"),
        # empty resolv.conf since the only mechanism to refer to the host (by name) is via /etc/hosts
        "//antlir/linux/vm/network:resolvconf",
        antlir1_feature.remove("/etc/hosts", must_exist = False),
        antlir1_feature.install("//antlir/linux/vm/network:etc-hosts", "/etc/hosts"),
    ]

network = struct(
    host = _host,
)
