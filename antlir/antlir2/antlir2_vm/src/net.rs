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

use crate::types::QemuDevice;
use crate::utils::format_command;
use crate::utils::log_command;

#[derive(Debug)]
/// Create and presents a virtual NIC to VM
pub(crate) struct VirtualNIC {
    /// ID of the NIC, start from 0. This affects the NIC name, its MAC and IP.
    id: usize,
    /// Max Combined Channels of the NIC. Multi-queue is disabled when set to 1
    max_combined_channels: usize,
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
    /// Create new VirtualNIC instance with assigned ID and max combined channels. The virtual NIC won't be
    /// created yet.
    pub(crate) fn new(id: usize, max_combined_channels: usize) -> Self {
        Self {
            id,
            max_combined_channels,
        }
    }

    /// Create the virtual NIC and assign an IP
    pub(crate) fn create_dev(&self) -> Result<()> {
        let dev_name = self.dev_name();
        let mut ip_command = vec!["tuntap", "add", "dev", &dev_name, "mode", "tap"];
        if self.max_combined_channels > 1 {
            ip_command.push("multi_queue");
        }
        self.ip_command(&ip_command)?;
        self.ip_command(&["link", "set", &dev_name, "up"])?;
        self.ip_command(&[
            "addr",
            "add",
            &self.ipv6_net(&self.host_ipv6_addr()),
            "dev",
            &dev_name,
        ])?;
        Ok(())
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

impl QemuDevice for VirtualNIC {
    fn qemu_args(&self) -> Vec<OsString> {
        [
            "-netdev",
            &format!(
                "tap,id=net{id},ifname={dev_name},script=no,downscript=no,queues={queues}",
                id = self.id,
                dev_name = self.dev_name(),
                queues = self.max_combined_channels,
            ),
            "-device",
            &format!(
                "virtio-net-pci,netdev=net{id},mac={mac},mq={mq},vectors={vectors}",
                id = self.id,
                mac = self.guest_mac(),
                mq = if self.max_combined_channels > 1 {
                    "on"
                } else {
                    "off"
                },
                // N for TX queues, N for RX queues, 2 for config, and 1 for possible control vq, where N = max_combined_channels
                // https://fburl.com/knmbw1a1
                vectors = self.max_combined_channels * 2 + 2,
            ),
        ]
        .iter()
        .map(|x| x.into())
        .collect()
    }
}

#[derive(Debug)]
pub(crate) struct VirtualNICs(Vec<VirtualNIC>);

impl VirtualNICs {
    pub(crate) fn new(count: usize, max_combined_channels: usize) -> Result<Self> {
        let nics: Result<Vec<_>> = (0..count)
            .map(|x| -> Result<VirtualNIC> {
                let nic = VirtualNIC::new(x, max_combined_channels);
                nic.create_dev()?;
                Ok(nic)
            })
            .collect();
        Ok(Self(nics?))
    }
}

impl QemuDevice for VirtualNICs {
    fn qemu_args(&self) -> Vec<OsString> {
        self.0.iter().flat_map(|x| x.qemu_args()).collect()
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn test_guest_mac() {
        assert_eq!(VirtualNIC::new(0, 1).guest_mac(), "00:00:00:00:00:01");
        assert_eq!(VirtualNIC::new(10, 2).guest_mac(), "00:00:00:00:00:0b");
        assert_eq!(VirtualNIC::new(100, 4).guest_mac(), "00:00:00:00:00:65");
        assert_eq!(VirtualNIC::new(1000, 16).guest_mac(), "00:00:00:00:03:e9");
    }

    #[test]
    fn test_ipv6_addr() {
        let nic = VirtualNIC::new(0, 1);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00::1/64");
        let nic = VirtualNIC::new(1, 2);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:1::1/64");
        let nic = VirtualNIC::new(10, 4);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:a::1/64");
        let nic = VirtualNIC::new(100, 16);
        assert_eq!(nic.ipv6_net(&nic.host_ipv6_addr()), "fd00:64::1/64");
    }

    #[test]
    fn test_qemu_args() {
        assert_eq!(
            VirtualNIC::new(0, 1).qemu_args().join(OsStr::new(" ")),
            "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no,queues=1 \
            -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01,mq=off,vectors=4"
        )
    }

    #[test]
    fn test_qemu_multiqueue_args() {
        assert_eq!(
            VirtualNIC::new(0, 128).qemu_args().join(OsStr::new(" ")),
            "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no,queues=128 \
            -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01,mq=on,vectors=258"
        )
    }

    #[test]
    fn test_nics_qemu_args() {
        let nics = VirtualNICs(vec![VirtualNIC::new(0, 1), VirtualNIC::new(1, 128)]);
        assert_eq!(
            nics.qemu_args().join(OsStr::new(" ")),
            "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no,queues=1 \
             -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01,mq=off,vectors=4 \
             -netdev tap,id=net1,ifname=vm1,script=no,downscript=no,queues=128 \
             -device virtio-net-pci,netdev=net1,mac=00:00:00:00:00:02,mq=on,vectors=258"
        )
    }
}
