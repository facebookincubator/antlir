#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import subprocess
from dataclasses import dataclass
from typing import Iterable

from antlir.unshare import Unshare


logger = logging.getLogger(__name__)


# The tap devices are created inside a network namespace, so it's fine for them
# all to have the same name.
TAPDEV = "vm0"

# MAC address for use in virtual machines.  Each VM is in its own network
# namespace, so this value is constant for all VMs.  Keep this in sync with
# bzl/constants.bzl.
VM_GUEST_MAC_ADDRESS = "00:00:00:00:00:01"


class TapError(Exception):
    pass


@dataclass(frozen=True)
class VmTap(object):
    """Functionality to manage a tap device to communicate with a VM guest
    over a network.
    VmTap is designed to operate within a network namespace, which absolves
    it of the need to clean up the interface after itself.

    VmTap currently requires sudo for some operations. Root is only required
    to setup the interface, afterwards QEMU can use it as an unprivileged
    user.

    NOTE: soon, a vm environment will be introduced that runs vms inside of
    a booted systemd-nspawn container.  This 'runtime' container will provide
    the necessary setup mechanism for this device, rendering the setup of
    /dev/net/tun moot.
    """

    netns: Unshare
    uid: int
    gid: int

    def __post_init__(self):
        self._ensure_dev_net_tun()
        logger.debug(f"creating tap device {TAPDEV} in namespace")
        try:
            subprocess.run(
                self.netns.nsenter_as_root(
                    "ip",
                    "tuntap",
                    "add",
                    "dev",
                    TAPDEV,
                    "mode",
                    "tap",
                    "user",
                    str(self.uid),
                    "group",
                    str(self.gid),
                ),
                check=True,
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
            )
            subprocess.run(
                self.netns.nsenter_as_root("ip", "link", "set", TAPDEV, "up"),
                check=True,
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
            )
            subprocess.run(
                self.netns.nsenter_as_root(
                    "ip", "addr", "add", self.host_ipv6, "dev", TAPDEV
                ),
                check=True,
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
            )
        except subprocess.CalledProcessError as e:
            raise TapError(f"Failed to setup tap device: {e.stderr}")

    def _ensure_dev_net_tun(self) -> None:
        # See class docblock, this should eventually be handled by the
        # environment before antlir ever gets invoked, but is necessary until
        # that day comes
        try:
            subprocess.run(
                [
                    "sudo",
                    "/bin/bash",
                    "-c",
                    """
    mkdir -p /dev/net
    mknod --mode=666 /dev/net/tun c 10 200
    [ -c /dev/net/tun ]
                """,
                ],
                check=True,
                capture_output=True,
            )
        except subprocess.CalledProcessError as e:
            raise TapError(f"Failed to mknod /dev/net/tun: {e.stderr}")

    @property
    def guest_mac(self) -> str:
        """
        Each vm is in its own network namespace, so the mac addresses for
        their interfaces are all the same. However, it still has to be
        deterministic (compared to allowing qemu to create a random one), so
        that the corresponding IPv6 link-local address is deterministic.
        """
        return VM_GUEST_MAC_ADDRESS

    @property
    def guest_ipv6_ll(self) -> str:
        return f"fe80::200:0ff:fe00:1%{TAPDEV}"

    @property
    def guest_ipv6(self) -> str:
        return "fd00::2"

    @property
    def host_ipv6(self) -> str:
        return "fd00::1/64"

    @property
    def qemu_args(self) -> Iterable[str]:
        return (
            "-netdev",
            f"tap,id=net0,ifname={TAPDEV},script=no,downscript=no",
            "-device",
            f"virtio-net-pci,netdev=net0,mac={self.guest_mac}",
        )
