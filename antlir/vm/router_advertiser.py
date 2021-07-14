#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# TODO: ideally we could just use something off-the-shelf like `radvd`, but
# doing that properly would really require running VMs inside a container so
# that systemd could handle all the namespaces and sidecar services

import time

from scapy.layers.inet6 import IPv6, ICMPv6ND_RA, ICMPv6NDOptPrefixInfo
from scapy.sendrecv import send


def start_router_advertisements(interval: float):
    def send_ras():
        while True:
            a = IPv6()
            a.dst = "ff02::1"
            b = ICMPv6ND_RA()
            e = ICMPv6NDOptPrefixInfo()
            e.prefixlen = 64
            e.prefix = "fd00::"
            pkt = a / b / e
            send(pkt, verbose=False)
            time.sleep(interval)

    send_ras()


if __name__ == "__main__":
    start_router_advertisements(interval=0.5)
