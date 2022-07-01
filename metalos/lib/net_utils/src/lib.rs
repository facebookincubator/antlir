/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::anyhow;
use anyhow::Result;
use nix::ifaddrs::getifaddrs;
use nix::net::if_::InterfaceFlags;
use nix::sys::socket::SockAddr;

/// This function returns the mac address of the first eth* interface which is currently in UP state.
/// This is used by the send-event logic (the event endpoint accept mac address, ip address or an asset ID).
pub fn get_mac() -> Result<String> {
    let addrs = getifaddrs().unwrap();
    for ifaddr in addrs {
        // For a list of InterfaceFlags see man(7) netdevice: https://man7.org/linux/man-pages/man7/netdevice.7.html
        if ifaddr.flags.contains(InterfaceFlags::IFF_UP)
            // IFF_RUNNING is supposed to reflect the operational status on a network interface,
            // rather than its administrative one.
            // To provide an example, an Ethernet interface may be brought UP by the administrator
            // (e.g. ifconfig eth0 up), but it will not be considered operational (i.e. RUNNING as per RFC2863)
            // if the cable is not plugged in.
            && ifaddr.flags.contains(InterfaceFlags::IFF_RUNNING)
            && !ifaddr.flags.contains(InterfaceFlags::IFF_LOOPBACK)
            && ifaddr.interface_name.starts_with("eth")
        {
            return match ifaddr.address {
                // check that address is a Datalink address (MAC) (nix::sys::socket::SockAddr::Link)
                Some(SockAddr::Link(address)) => Ok(address.to_string()),
                _ => {
                    continue;
                }
            };
        }
    }
    Err(anyhow!(
        "Can't find any mac address in this environment. Are we in a an isolated environment?"
    ))
}

#[cfg(test)]
mod tests {
    use super::get_mac;
    use anyhow::Result;

    #[test]
    fn test_get_mac() -> Result<()> {
        let mac = get_mac()?;
        // TODO: improve me, perhaps compare with output of
        //
        // $ ip addr show dev eth0 | awk '/ether/ {print $2}'
        // b0:26:28:b4:0a:be
        //
        // any other better idea?
        assert_ne!(mac, "");
        Ok(())
    }
}
