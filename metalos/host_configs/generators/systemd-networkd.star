
# The UseAutonomousPrefix=false config was added because without this our machines would
# use IPv6 privacy settings and we would send from an IP that rootcanal didn't recognize
# and so our request for certificates would be denied
NETWORK_TEMPLATE = metalos.template("""
[Match]
MACAddress={{mac}}

[Network]
{{#each ipv6_addrs}}
Address={{this.addr}}/{{this.prefix}}
{{/each~}}
{{~#each ipv4_addrs}}
Address={{this}}
{{/each~}}
Domains={{#each search}}{{this}} {{/each}}
IPv6AcceptRA={{accept_ras}}

[IPv6AcceptRA]
UseAutonomousPrefix=false
""")

LINK_TEMPLATE = metalos.template("""
[Match]
MACAddress={{mac}}

[Link]
NamePolicy=
Name={{name}}
""")


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

    # We only want to accept RA (thus default route) on our primary interface
    primary_mac = prov.identity.network.primary_interface.mac

    for i, iface in enumerate(prov.identity.network.interfaces):
        accept_ras = "yes" if iface.mac == primary_mac else "no"
        ipv4_addrs = [a for a in iface.addrs if "." in a]
        ipv6_addrs = [struct(addr=a, prefix="64") for a in iface.addrs if ":" in a]
        unit = NETWORK_TEMPLATE(accept_ras=accept_ras, mac=iface.mac, ipv6_addrs=ipv6_addrs, ipv4_addrs=ipv4_addrs, search=search)
        network_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.network".format(iface.name or i), contents=unit)]
        if iface.name:
            unit = LINK_TEMPLATE(mac=iface.mac, name=iface.name)
            link_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.link".format(iface.name), contents=unit)]

    return metalos.Output(
        files=network_units + link_units,
    )
