/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This file contains structs that create network interfaces for the VM. All code
//! here should only be run inside a container.

use std::ffi::OsString;
use std::net::Ipv6Addr;
use std::process::Command;

use thiserror::Error;

use crate::utils::format_command;
use crate::utils::log_command;

#[derive(Debug)]
/// Create and presents a virtual NIC to VM
pub(crate) struct VirtualNIC {
    /// ID of the NIC, start from 0. This affects the NIC name, its MAC and IP.
    id: usize,
}

#[derive(Error, Debug)]
pub(crate) enum VirtualNICError {
    #[error(transparent)]
    IPCmdExecError(#[from] std::io::Error),
    #[error("Error from command: `{0}` ")]
    IPCmdReturnError(String),
}

type Result<T> = std::result::Result<T, VirtualNICError>;

impl VirtualNIC {
    /// Create new VirtualNIC instance with assigned ID. The virtual NIC won't be
    /// created yet.
    pub(crate) fn new(id: usize) -> Self {
        Self { id }
    }

    /// Create the virtual NIC and assign an IP
    pub(crate) fn create_dev(&self) -> Result<()> {
        self.ip_command(&["tuntap", "add", "dev", &self.dev_name(), "mode", "tap"])?;
        self.ip_command(&["link", "set", &self.dev_name(), "up"])?;
        self.ip_command(&[
            "addr",
            "add",
            &self.ipv6_net(&self.host_ipv6_addr()),
            "dev",
            &self.dev_name(),
        ])?;
        Ok(())
    }

    /// Qemu args to append
    pub(crate) fn qemu_args(&self) -> Vec<OsString> {
        [
            "-netdev",
            &format!(
                "tap,id=net{id},ifname={dev_name},script=no,downscript=no",
                id = self.id,
                dev_name = self.dev_name(),
            ),
            "-device",
            &format!(
                "virtio-net-pci,netdev=net{id},mac={mac}",
                id = self.id,
                mac = self.guest_mac(),
            ),
        ]
        .iter()
        .map(|x| x.into())
        .collect()
    }

    /// Name for the virtual interface
    fn dev_name(&self) -> String {
        format!("vm{}", self.id)
    }

    /// MAC needs to be predicatable so the VM can pre-configure its network.
    /// The MAC address is formatted `self.id`.
    pub(crate) fn guest_mac(&self) -> String {
        format!("{:012x}", self.id + 1)
            .split("")
            .filter(|x| !x.is_empty())
            .enumerate()
            .fold(String::new(), |acc, (i, c)| match i {
                0 => c.to_string(),
                _ if i % 2 == 0 => acc + ":" + c,
                _ => acc + c,
            })
    }

    /// Host side IP address. It's fd00:<id>::1.
    fn host_ipv6_addr(&self) -> Ipv6Addr {
        let mut ip: u128 = 0xfd00 << (128 - 16);
        ip += ((self.id as u128) << (128 - 32)) + 1;
        Ipv6Addr::from(ip)
    }

    /// We always use /64
    fn ipv6_net(&self, addr: &Ipv6Addr) -> String {
        format!("{}/64", addr)
    }

    fn ip_command(&self, args: &[&str]) -> Result<()> {
        let mut command = Command::new("ip");
        log_command(command.args(args))
            .status()?
            .success()
            .then_some(())
            .ok_or(VirtualNICError::IPCmdReturnError(format_command(&command)))
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn test_guest_mac() {
        assert_eq!(VirtualNIC::new(0).guest_mac(), "00:00:00:00:00:01");
        assert_eq!(VirtualNIC::new(10).guest_mac(), "00:00:00:00:00:0b");
        assert_eq!(VirtualNIC::new(100).guest_mac(), "00:00:00:00:00:65");
        assert_eq!(VirtualNIC::new(1000).guest_mac(), "00:00:00:00:03:e9");
    }

    #[test]
    fn test_ipv6_addr() {
        let nic = VirtualNIC::new(0);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00::1/64");
        let nic = VirtualNIC::new(1);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:1::1/64");
        let nic = VirtualNIC::new(10);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:a::1/64");
        let nic = VirtualNIC::new(100);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:64::1/64");
    }

    #[test]
    fn test_qemu_args() {
        assert_eq!(
            VirtualNIC::new(0).qemu_args().join(OsStr::new(" ")),
            "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no \
            -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01"
        )
    }
}
