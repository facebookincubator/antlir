#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import os
import subprocess
from dataclasses import dataclass
from itertools import zip_longest
from typing import Iterable

from antlir.unshare import Unshare


logger = logging.getLogger(__name__)


def grouper(iterable, n, fillvalue=None):
    "Collect data into fixed-length chunks or blocks"
    # grouper('ABCDEFG', 3, 'x') --> ABC DEF Gxx"
    args = [iter(iterable)] * n
    return zip_longest(*args, fillvalue=fillvalue)


# The tap devices are created inside a network namespace, so it's fine for them
# all to have the same name.
TAPDEV = "vm0"


@dataclass(frozen=True)
class VmTap(object):
    """Functionality to manage a tap device to communicate with a VM guest
    over a network.
    VmTap is designed to operate within a network namespace, which absolves
    it of the need to clean up the interface after itself.

    VmTap currently requires sudo for some operations. Root is only required
    to setup the interface, afterwards QEMU can use it as an unprivileged
    user.
    TODO: on devservers this sudo requirement is fine, but the end goal is to
    allow completely rootless operation, in which case we will expect some
    kind of setup code (eg twagent, docker) to create a network namespace and
    run vmtest with CAP_NET_ADMIN to be able to configure it.
    """

    netns: Unshare
    uid: int
    gid: int

    def __post_init__(self):
        self._ensure_dev_net_tun()
        logger.debug(f"creating tap device {TAPDEV} in namespace")
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

    def _ensure_dev_net_tun(self) -> None:
        # See class docblock, this should eventually be handled by the
        # environment before antlir ever gets invoked, but is necessary until
        # that day comes
        try:
            subprocess.run(
                self.netns.nsenter_as_root("stat", "/dev/net/tun"),
                capture_output=True,
                check=True,
            )
        except subprocess.CalledProcessError:
            logger.warning("/dev/net/tun does not exist, creating it")
            os.makedirs("/dev/net", exist_ok=True)
            subprocess.run(
                self.netns.nsenter_as_root(
                    "mknod", "/dev/net/tun", "c", "10", "200"
                ),
                check=True,
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
            )

    @property
    def guest_mac(self) -> str:
        """
        Each vm is in its own network namespace, so the mac addresses for
        their interfaces are all the same. However, it still has to be
        deterministic (compared to allowing qemu to create a random one), so
        that the corresponding IPv6 link-local address is deterministic.
        """
        return "00:00:00:00:00:01"

    @property
    def guest_ipv6_ll(self) -> str:
        parts = self.guest_mac.split(":")

        # fffe gets added into the middle of the mac address
        parts.insert(3, "ff")
        parts.insert(4, "fe")
        # invert U/L bit
        parts[0] = "{:02x}".format(int(parts[0], 16) ^ 2)

        ll = ":".join("".join(g) for g in grouper(parts, 2))
        return f"fe80::{ll}%{TAPDEV}"

    @property
    def qemu_args(self) -> Iterable[str]:
        return (
            "-netdev",
            f"tap,id=net0,ifname={TAPDEV},script=no,downscript=no",
            "-device",
            f"virtio-net-pci,netdev=net0,mac={self.guest_mac}",
        )
