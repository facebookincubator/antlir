---
id: network
title: MetalNework
---

## MetalNetworking

tl;dr MetalOS is all in on systemd-networkd for layered image network config.

[networkd](https://www.freedesktop.org/software/systemd/man/systemd-networkd.service.html) is MetalOS’s
one stop network configuration tool. It’s an [ini](https://en.wikipedia.org/wiki/INI_file) file based
configuration system dedicated to the hosts networking stack that has similar layout to systemd units
and tooling. networkctl is your networkd CLI tool, kind of synonymous to systemctl, but missing a lot
of features.

* [networkctl](https://www.freedesktop.org/software/systemd/man/networkctl.html)


## MetalOS + networkd

All networkd configurations are starlark templates that use hostconfig data to be rendered by
`//metalos/host_configs/evalctx/...` code.

- All networkd config shipped with MetalOS lives in
    - **/usr/lib/systemd/network**
- Antlir/MetalOS images/layers support network customizations via "drop in" `.d` networkd support

### Handy Man Pages for networkd

- [networkd Service:](https://www.freedesktop.org/software/systemd/man/systemd-networkd.service.html)
    - The systemd service itself
- [Link files](https://www.freedesktop.org/software/systemd/man/systemd.link.html)
    - Control link layer (layer 2) properties for a device - e.g. Change mac address or interface alias/name
- [Netdev files](https://www.freedesktop.org/software/systemd/man/systemd.netdev.html)
    - Create virtual interfaces - e.g. vlan or bond interfaces
- [Network files](https://www.freedesktop.org/software/systemd/man/systemd.network.html)
    - Control network layer (layer 3) properties for a device - e.g. Add routes or change MTU


## MetalOS Networking

All metalos images get the following:

- NIC renaming to ensure host device names match Host Config at boot
  - Via `.link` files that generate `udev` rules
- Static IPv6 prefix per NIC
    - Requires static prefix to be in host config
    - Addresses are sourced from `host_config.identity.network.interfaces`
- Default routes to rack switch
- RAs disabled, thus, DHCPv6 client is disabled


### Layered / Layers Networking

Different host roles and prefixes are going to need some networking tweaking.
To provide that capability MetalOS needs a way to layer network configuration.

We plan on using networkd’s order of configuration preference and "drop-in" `.d` directories.

- This is still up for debate but we will update documentation when this is all finalized

networkd's order preference is:

1. /etc/systemd/network
2. /lib/systemd/network
3. /run/systemd/network  ← meant for vendor usage - Leaving for container tweaks
4. /usr/lib/systemd/network ← meant for vendor usage - MetalOS base config + layer overrides will be here

All configuration build by networkd automation will live in /usr/lib/systemd/network.

### Changing base MetalOS network settings

To make changes today we still have everything in `systemd-networkd.star`.

#### Testing changes

To test your changes we have Rust unittests that check the star file generated rendered files.

To run the tests:

-  `buck2 test metalos/host_configs/evalctx:evalctx`

Running the MetalOS VM is also a good way to test your changes.
This needs a lot more improvement tho! Diffs welcome.

- `buck2 run //antlir/vm:default`

### Adding overrides to your image

TODO: Complete when we've decided / implemented this
