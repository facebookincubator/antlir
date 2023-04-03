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
    index: int

    def __post_init__(self):
        self._ensure_dev_net_tun()
        logger.debug(f"creating tap device {self.tapdev} in namespace")
        try:
            subprocess.run(
                self.netns.nsenter_as_root(
                    "ip",
                    "tuntap",
                    "add",
                    "dev",
                    self.tapdev,
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
                self.netns.nsenter_as_root("ip", "link", "set", self.tapdev, "up"),
                check=True,
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
            )
            subprocess.run(
                self.netns.nsenter_as_root(
                    "ip", "addr", "add", self.host_ipv6, "dev", self.tapdev
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
    def tapdev(self) -> str:
        # The tap devices are created inside a network namespace, so it's fine for them
        # all to have the same name.
        return f"vm{self.index}"

    @property
    def guest_mac(self) -> str:
        """
        Each vm is in its own network namespace, so the mac addresses for
        their interfaces are all the same. However, it still has to be
        deterministic (compared to allowing qemu to create a random one), so
        that the corresponding IPv6 link-local address is deterministic.
        """
        # MAC address for use in virtual machines.  Each VM is in its own network
        # namespace, so this value is constant for all VMs.  Keep this in sync with
        # bzl/constants.bzl.
        return "00:00:00:00:00:{0:02d}".format(self.index + 1)

    @property
    def guest_ipv6_ll(self) -> str:
        # + 1 so we start at 1
        return f"fe80::200:0ff:fe00:{self.index + 1}%{self.tapdev}"

    @property
    def guest_ipv6(self) -> str:
        # Start at 2 so 0 (first) VM gets same addressing before multi NIC support
        return f"fd00::{2+self.index}"

    @property
    def host_ipv6(self) -> str:
        return "fd00::1/64"

    @property
    def qemu_args(self) -> Iterable[str]:
        return (
            "-netdev",
            f"tap,id=net0,ifname={self.tapdev},script=no,downscript=no",
            "-device",
            f"virtio-net-pci,netdev=net0,mac={self.guest_mac}",
        )
