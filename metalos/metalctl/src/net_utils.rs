/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Result};
use nix::ifaddrs::getifaddrs;
use nix::net::if_::InterfaceFlags;
use nix::sys::socket::SockAddr;

/// This function returns the mac address of an eth* interface which is currently in UP state.
pub fn get_mac() -> Result<String> {
    let addrs = getifaddrs().unwrap();
    for ifaddr in addrs {
        // For a list of InterfaceFlags see man(7) netdevice: https://man7.org/linux/man-pages/man7/netdevice.7.html
        if ifaddr.flags.contains(InterfaceFlags::IFF_UP)
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
        "can't find any mac address, we should never get here!"
    ))
}
