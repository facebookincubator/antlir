
# The UseAutonomousPrefix=false config was added because without this our machines would
# use IPv6 privacy settings and we would send from an IP that rootcanal didn't recognize
# and so our request for certificates would be denied
NETWORK_TEMPLATE = metalos.template("""
[Match]
MACAddress={{mac}}

[Network]

Domains={{#each search}}{{this}} {{/each}}
IPv6AcceptRA=no

[IPv6AcceptRA]
UseMTU=false
UseAutonomousPrefix=false
UseOnLinkPrefix=false
DHCPv6Client=false

{{#each ipv6_addrs}}
[Address]
Address={{this.addr}}/{{this.prefix}}
PreferredLifetime={{this.prefered_lifetime}}
{{/each~}}
{{~#each ipv4_addrs}}
[Address]
Address={{this}}
{{/each~}}

{{#each routes}}
[Route]
Gateway={{this.gw}}
Source={{this.src}}
Destination={{this.dest}}
Metric={{this.metric}}
{{/each~}}

""")

LINK_TEMPLATE = metalos.template("""
[Match]
MACAddress={{mac}}

[Link]
NamePolicy=
Name={{name}}
MTUBytes={{mtu}}
RequiredForOnline={{required_for_online}}
""")

ADDR_PRIMARY    = 0
ADDR_SECONDARY  = 1
ADDR_DEPRECATED = 2

INTFS_FRONTEND  = 0
INTFS_BACKEND   = 1

FE_GW = "fe80::face:b00c"
BE_GW = "fe80::face:b00b"

DEFAULT_MTU = "1500"
BE_MTU = "4200"

# Automatically add search domains for all the domains after the host
# itself, if any (ex: host001.01.abc0.facebook.com -> 01.abc0.facebook.com, abc0.facebook.com, facebook.com)
def auto_search_domains(name: str.type) -> [str.type]:
    search = []
    if "." in name:
        split = name.split(".")[1:]
        tld = split[-1]
        domain = tld
        split = reversed(split[:-1])
        for sub in split:
            domain = sub + "." + domain
            search.append(domain)
    return reversed(search)


def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    search = auto_search_domains(prov.identity.hostname) + prov.identity.network.dns.search_domains
    network_units = []
    link_units = []

    for i, iface in enumerate(prov.identity.network.interfaces):
        ipv4_addrs = [a.addr for a in iface.structured_addrs if "." in a.addr]
        ipv6_addrs = [struct(
                            addr=a.addr,
                            prefix=a.prefix_length,
                            prefered_lifetime="forever" if a.mode != ADDR_DEPRECATED else "0")
                        for a in iface.structured_addrs if ":" in a.addr]
        routes = []
        # Create interface source routes for all FE and BE interface on host.
        if iface.interface_type == INTFS_FRONTEND or iface.interface_type == INTFS_BACKEND and iface.essential != True:
            routes = [struct(
                        gw=FE_GW if iface.interface_type == INTFS_FRONTEND else BE_GW,
                        dest="::/0",
                        metric="1024",
                        src=a.addr)
                    for a in iface.structured_addrs if ":" in a.addr and a.mode == ADDR_PRIMARY]
        # High priority route for essential / primary interface.
        if iface.essential == True:
            routes = [struct(
                        gw=FE_GW,
                        dest="::/0",
                        metric="10",
                        src="::/0")]

        unit = NETWORK_TEMPLATE(mac=iface.mac, ipv6_addrs=ipv6_addrs, ipv4_addrs=ipv4_addrs, routes=routes, search=search)
        network_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.network".format(iface.name or i), contents=unit)]
        if iface.name:
            mtu = BE_MTU if iface.interface_type == INTFS_BACKEND else DEFAULT_MTU
            required_for_online = "yes" if iface.essential == True else "no"
            unit = LINK_TEMPLATE(mac=iface.mac, name=iface.name, mtu=mtu, required_for_online=required_for_online)
            link_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.link".format(iface.name), contents=unit)]

    return metalos.Output(
        files=network_units + link_units,
    )
