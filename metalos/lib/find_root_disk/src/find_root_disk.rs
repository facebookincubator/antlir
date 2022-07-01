/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This file works for now but when we want to support multiple devices we should probably switch
// to something like: third-party/rust/fixups/libparted-sys/fixups.toml

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use udev::Enumerator;

use metalos_disk::DiskDevPath;

const IGNORED_PREFIXES: &[&str] = &["/sys/devices/virtual/"];

// This is copied from:
// https://github.com/util-linux/util-linux/blob/master/misc-utils/lsblk-properties.c#L108-L118
const POSSIBLE_SERIAL_PROPERTIES: &[&str] = &[
    "SCSI_IDENT_SERIAL",
    "ID_SCSI_SERIAL",
    "ID_SERIAL_SHORT",
    "ID_SERIAL",
];

pub trait DiskPath {
    fn dev_node(&self) -> Result<DiskDevPath>;
    fn sys_path(&self) -> &Path;
}

impl DiskPath for udev::Device {
    fn dev_node(&self) -> Result<DiskDevPath> {
        self.devnode()
            .map(|p| DiskDevPath(p.to_path_buf()))
            .context(format!(
                "No dev path found for device at {:?}",
                self.sys_path()
            ))
    }

    fn sys_path(&self) -> &Path {
        self.syspath()
    }
}

/// A trait to enumerate all disks on the machine. This is used to logically split the
/// responsibility of finding all disks from picking the right one. They are linked together
/// by the DiskPath trait above which is used as the common language for information about disks.
pub trait DiskDiscovery {
    type Output: DiskPath;

    /// This returns all valid disk devices on the system that you can pick from
    fn discover_devices() -> Result<Vec<Self::Output>>;
}

/// Responsible for finding the root disk on the system. There are potentially a bunch of different
/// ways of doing this and so you must provide the `Output` type that we can get some minimum info from
/// that is needed for the other functions or the common usages of the library so we don't get too
/// bound to the `udev` crate.
pub trait FindRootDisk {
    /// The type we return for the root disk. This is a compromise for having the interface be
    /// bound to the `udev` crate but still being able to use other types if you want too as most
    /// users will only want to be able to get the path from the struct.
    type Output: DiskPath;

    /// The source of disks on the host
    type Discovery: DiskDiscovery<Output = Self::Output>;

    /// This is the main entry point for getting the root disk for the finder
    /// and is likely the only function you will need to use. It's also unlikely
    /// you will want to overwirte this function if you are making your own impl
    /// of this trait.
    fn get_root_device(&self) -> Result<Self::Output> {
        let devices = Self::Discovery::discover_devices().context("failed to discover devices")?;

        let devices = self
            .filter_unusable(devices)
            .context("failed to filter usable devices")?;

        self.find_root_disk(devices)
            .context("Failed to select suitable root device")
    }

    /// This attempts to filter out other block devices like /dev/loop and /dev/ram devices.
    /// It's currently pretty dumb so it's in it's own function in case you want to remove or
    /// overwite this functionality for your type
    fn filter_unusable(&self, devices: Vec<Self::Output>) -> Result<Vec<Self::Output>> {
        Ok(devices
            .into_iter()
            .filter(|device| {
                !IGNORED_PREFIXES
                    .iter()
                    .any(|p| device.sys_path().starts_with(p))
            })
            .collect())
    }

    /// The main (and probably only) function you should impl if you are building your own
    /// finder type. It takes in all devices (after filtering) and must return the one device
    /// that we are going to use as our root device.
    fn find_root_disk(&self, devices: Vec<Self::Output>) -> Result<Self::Output>;
}

pub struct UdevDiscovery {}
impl DiskDiscovery for UdevDiscovery {
    type Output = udev::Device;

    fn discover_devices() -> Result<Vec<Self::Output>> {
        let mut enumerator = Enumerator::new().context("failed to build enumerator")?;
        enumerator
            .match_property("DEVTYPE", "disk")
            .context("failed to add property filter")?;

        let devices = enumerator
            .scan_devices()
            .context("failed to scan devices")?;
        Ok(devices.into_iter().collect())
    }
}

/// Finds the root disk assuming there is only a single device on the system
/// it will error out if there are multiple disks.
pub struct SingleDiskFinder {}

impl SingleDiskFinder {
    pub fn new() -> Self {
        Self {}
    }
}

impl FindRootDisk for SingleDiskFinder {
    type Output = udev::Device;
    type Discovery = UdevDiscovery;

    fn find_root_disk(&self, devices: Vec<Self::Output>) -> Result<Self::Output> {
        match devices.len() {
            0 => Err(anyhow!("Found no valid root devices")),
            1 => Ok(devices.into_iter().next().unwrap()),
            n => Err(anyhow!(
                "Found {} possible root devices, expected 1. Found: {:?}",
                n,
                devices
            )),
        }
    }
}

/// Finds the root disk with the given serial number. This has a microscopic chance of
/// failure if drives from different vendors with same serial number get installed
/// into same machine.
pub struct SerialDiskFinder {
    serial: String,
}

impl SerialDiskFinder {
    pub fn new(serial: String) -> Self {
        Self { serial }
    }

    fn get_serial(device: &udev::Device) -> Result<Option<&str>> {
        for prop in POSSIBLE_SERIAL_PROPERTIES {
            if let Some(value) = device.property_value(prop) {
                return Ok(Some(
                    value
                        .to_str()
                        .context("failed to convert property from OsStr to str")?,
                ));
            }
        }
        Ok(None)
    }
}

impl FindRootDisk for SerialDiskFinder {
    type Output = udev::Device;
    type Discovery = UdevDiscovery;

    fn find_root_disk(&self, devices: Vec<Self::Output>) -> Result<Self::Output> {
        let mut seen = Vec::new();
        for device in devices {
            if let Some(serial) = Self::get_serial(&device)? {
                if serial.trim() == self.serial.trim() {
                    return Ok(device);
                }
                seen.push(serial.to_string());
            }
        }

        Err(anyhow!(
            "None of the provided devices matched the serial {} found: {:?}",
            self.serial,
            seen
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metalos_macros::vmtest;
    use std::path::Path;

    fn test_finder<T: FindRootDisk>(finder: T) -> Result<()> {
        let dev = finder
            .get_root_device()
            .context("Failed to select root device")?;
        assert_eq!(
            dev.dev_node()
                .context("expected to find devnode for returned device")?,
            DiskDevPath("/dev/vda".into())
        );

        Ok(())
    }

    #[vmtest]
    fn test_get_single_root_device() -> Result<()> {
        test_finder(SingleDiskFinder::new())
    }

    #[vmtest]
    fn test_get_serial_root_device() -> Result<()> {
        // We still only have a single disk but this at least tests that we can find
        // the root disk and the VM is setup right.
        test_finder(SerialDiskFinder::new("ROOT_DISK_SERIAL".to_string()))
    }
}
