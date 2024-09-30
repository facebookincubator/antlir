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
use std::ops::Index;
use std::ops::IndexMut;
use std::path::PathBuf;
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
    /// Dump interface traffic to this file. This is not supported for multi-queue NICs.
    dump_file: Option<PathBuf>,
}

#[derive(Error, Debug)]
pub(crate) enum VirtualNICError {
    #[error(transparent)]
    IPCmdExecError(#[from] std::io::Error),
    #[error("Error from command: `{0}` ")]
    IPCmdReturnError(String),
    #[error("Traffic is not dumpable: `{0}` ")]
    TrafficDumpingNotSupported(String),
}

type Result<T> = std::result::Result<T, VirtualNICError>;

impl VirtualNIC {
    /// Create new VirtualNIC instance with assigned ID and max combined channels. The virtual NIC won't be
    /// created yet.
    pub(crate) fn new(id: usize, max_combined_channels: usize) -> Self {
        Self {
            id,
            max_combined_channels,
            dump_file: None,
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

    /// ID of the virtual interface
    pub(crate) fn dev_id(&self) -> String {
        format!("net{}", self.id)
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

    /// Set the file to dump interface traffic to.
    /// Set to None to disable dumping traffic.
    pub(crate) fn try_dump_file(&mut self, path: Option<PathBuf>) -> Result<&mut Self> {
        if self.max_combined_channels > 1 {
            return Err(VirtualNICError::TrafficDumpingNotSupported(
                "Can not dump traffic for multi-queue NIC: https://fburl.com/dmblggwc".into(),
            ));
        }
        self.dump_file = path;
        Ok(self)
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
        let mut vec: Vec<_> = [
            "-netdev",
            &format!(
                "tap,id={dev_id},ifname={dev_name},script=no,downscript=no,queues={queues}",
                dev_id = self.dev_id(),
                dev_name = self.dev_name(),
                queues = self.max_combined_channels,
            ),
            "-device",
            &format!(
                "virtio-net-pci,netdev={dev_id},mac={mac},mq={mq},vectors={vectors}",
                dev_id = self.dev_id(),
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
        .collect();
        if let Some(path) = &self.dump_file {
            vec.extend(
                [
                    "-object",
                    &format!(
                        "filter-dump,id=dump0,netdev={},file={}",
                        self.dev_id(),
                        path.to_string_lossy()
                    ),
                ]
                .iter()
                .map(|x| x.into())
                .collect::<Vec<_>>(),
            );
        }

        vec
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

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl Index<usize> for VirtualNICs {
    type Output = VirtualNIC;
    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

impl IndexMut<usize> for VirtualNICs {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.0.index_mut(index)
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
    fn test_qemu_args_with_dump_file() {
        let dump_file = PathBuf::from("/tmp/dump");
        let mut nic = VirtualNIC::new(0, 1);
        nic.try_dump_file(Some(dump_file.clone())).unwrap();

        assert_eq!(
            nic.qemu_args().join(OsStr::new(" ")),
            format!(
                "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no,queues=1 \
            -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01,mq=off,vectors=4 \
            -object filter-dump,id=dump0,netdev=net0,file={}",
                dump_file.to_string_lossy()
            )
            .as_str()
        )
    }

    #[test]
    // This test is to make sure that the dump file is not added to the qemu args when it's not supported (multi-queue nics)
    fn test_qemu_args_with_dump_file_not_supported() {
        let dump_file = PathBuf::from("/tmp/dump");
        let mut nic = VirtualNIC::new(0, 2);
        assert!(nic.try_dump_file(Some(dump_file.clone())).is_err());

        assert_eq!(
            nic.qemu_args().join(OsStr::new(" ")),
            "-netdev tap,id=net0,ifname=vm0,script=no,downscript=no,queues=2 \
            -device virtio-net-pci,netdev=net0,mac=00:00:00:00:00:01,mq=on,vectors=6"
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

    #[test]
    fn test_nics_access() {
        let nics = VirtualNICs(vec![VirtualNIC::new(0, 1), VirtualNIC::new(1, 128)]);
        assert_eq!(nics.len(), 2);

        assert_eq!(nics[1].dev_id(), "net1");
    }

    #[test]
    #[should_panic(expected = "index out of bounds: the len is 1 but the index is 1")]
    fn test_nics_outofbounds_access() {
        let nics = VirtualNICs(vec![VirtualNIC::new(0, 1)]);
        nics[1].dev_id();
    }
}
