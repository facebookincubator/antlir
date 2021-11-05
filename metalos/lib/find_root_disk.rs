/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This file works for now but when we want to support multiple devices we should probably switch
// to something like: third-party/rust/fixups/libparted-sys/fixups.toml

use anyhow::{anyhow, Context, Result};
use udev::{Device, Enumerator};

const IGNORED_PREFIXES: &[&str] = &["/sys/devices/virtual/"];

// This is copied from:
// https://github.com/util-linux/util-linux/blob/master/misc-utils/lsblk-properties.c#L108-L118
const POSSIBLE_SERIAL_PROPERTIES: &[&str] = &[
    "SCSI_IDENT_SERIAL",
    "ID_SCSI_SERIAL",
    "ID_SERIAL_SHORT",
    "ID_SERIAL",
];

/// Responsible for finding the root disk on the system. There are potentially a bunch of different
/// ways of doing this however this interface is reasonably fixed to the `udev` crate because we
/// return it's Device type. If this becomes an issue we can wrap a generic `Device` struct in here
/// but that felt overkill for now.
pub trait FindRootDisk {
    /// This is the main entry point for getting the root disk for the finder
    /// and is likely the only function you will need to use. It's also unlikely
    /// you will want to overwirte this function if you are making your own impl
    /// of this trait.
    fn get_root_device(&self) -> Result<Device> {
        let devices = self
            .discover_devices()
            .context("failed to discover devices")?;

        let devices = self
            .filter_unusable(devices)
            .context("failed to filter usable devices")?;

        self.find_root_disk(devices)
            .context("Failed to select suitable root device")
    }

    /// This returns all valid "disk" (DEVTYPE == "disk") devices that are found on the
    /// system with no other filtering applied.
    fn discover_devices(&self) -> Result<Vec<Device>> {
        let mut enumerator = Enumerator::new().context("failed to build enumerator")?;
        enumerator
            .match_property("DEVTYPE", "disk")
            .context("failed to add property filter")?;

        let devices = enumerator
            .scan_devices()
            .context("failed to scan devices")?;
        Ok(devices.into_iter().collect())
    }

    /// This attempts to filter out other block devices like /dev/loop and /dev/ram devices.
    /// It's currently pretty dumb so it's in it's own function in case you want to remove or
    /// overwite this functionality for your type
    fn filter_unusable(&self, devices: Vec<Device>) -> Result<Vec<Device>> {
        Ok(devices
            .into_iter()
            .filter(|device| {
                !IGNORED_PREFIXES
                    .iter()
                    .any(|p| device.syspath().starts_with(p))
            })
            .collect())
    }

    /// The main (and probably only) function you should impl if you are building your own
    /// finder type. It takes in all devices (after filtering) and must return the one device
    /// that we are going to use as our root device.
    fn find_root_disk(&self, devices: Vec<Device>) -> Result<Device>;
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
    fn find_root_disk(&self, devices: Vec<Device>) -> Result<Device> {
        match devices.len() {
            0 => Err(anyhow!("Found no valid root devices")),
            1 => Ok(devices.into_iter().next().unwrap()),
            n => Err(anyhow!(
                "Found {} possible root devices which isn't yet supported. Found: {:?}",
                n,
                devices
            )),
        }
    }
}

/// Finds the root disk with the given serial number. The idea of this is that we can
/// lookup the device's serial number in advance and provide this as a kernel parameter
/// so there is no ambiguity or searching logic the host has to do itself.
pub struct SerialDiskFinder {
    serial: String,
}

impl SerialDiskFinder {
    pub fn new(serial: String) -> Self {
        Self { serial }
    }

    fn get_serial(device: &Device) -> Result<Option<&str>> {
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
    fn find_root_disk(&self, devices: Vec<Device>) -> Result<Device> {
        for device in devices {
            if let Some(serial) = Self::get_serial(&device)? {
                if serial == self.serial {
                    return Ok(device);
                }
            }
        }

        Err(anyhow!("None of the provided devices matched the serial"))
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
            dev.devnode()
                .context("expected to find devnode for returned device")?,
            Path::new("/dev/vda")
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
