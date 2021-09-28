NETWORK_TEMPLATE = metalos.template("""
[Match]
MACAddress={{mac}}

[Network]
{{~#each ipv6_addrs}}
Address={{this.addr}}/{{this.prefix}}
{{/each~}}
{{~#each ipv4_addrs}}
Address={{this}}
{{/each~}}
Domains={{#each search}}{{this}} {{/each}}
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


def generator(host: metalos.Host) -> metalos.GeneratorOutput.type:
    search = auto_search_domains(host.hostname) + host.network.dns.search_domains
    network_units = []
    link_units = []
    for i, iface in enumerate(host.network.interfaces):
        ipv4_addrs = [a for a in iface.addrs if "." in a]
        ipv6_addrs = [struct(addr=a, prefix="64") for a in iface.addrs if ":" in a]
        unit = NETWORK_TEMPLATE(mac=iface.mac, ipv6_addrs=ipv6_addrs, ipv4_addrs=ipv4_addrs, search=search)
        network_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.network".format(iface.name or i), contents=unit)]
        if iface.name:
            unit = LINK_TEMPLATE(mac=iface.mac, name=iface.name)
            link_units += [metalos.file(path="/etc/systemd/network/00-metalos-{}.link".format(iface.name), contents=unit)]

    return metalos.GeneratorOutput(
        files=network_units + link_units,
    )
