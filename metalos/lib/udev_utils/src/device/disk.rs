/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::ffi::OsString;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use crate::device::Device;
use crate::device::DeviceType;
use crate::device::PropertyError;
use crate::device::SpecializationError;
use crate::device::SpecificDevice;

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    device: Device,
    path: PathBuf,
    serial: Option<OsString>,
}

impl Disk {
    /// Path in /dev
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn serial(&self) -> Option<&OsStr> {
        self.serial.as_deref()
    }
}

impl SpecificDevice for Disk {}

impl From<Disk> for Device {
    fn from(d: Disk) -> Self {
        d.device
    }
}

impl TryFrom<Device> for Disk {
    type Error = SpecializationError;

    fn try_from(device: Device) -> std::result::Result<Self, SpecializationError> {
        match device.device_type() {
            DeviceType::Disk => {
                let path = device
                    .dev_path
                    .clone()
                    .ok_or(SpecializationError::Property {
                        property: "devnode",
                        error: PropertyError::Missing,
                    })?;

                // Because of course there are a ton of these... Copied from:
                // https://github.com/util-linux/util-linux/blob/8883f037466a5534554d7d9114aceb740295ef20/misc-utils/lsblk-properties.c#L118,L124
                let serial = [
                    OsStr::new("SCSI_IDENT_SERIAL"),
                    OsStr::new("ID_SCSI_SERIAL"),
                    OsStr::new("ID_SERIAL_SHORT"),
                    OsStr::new("ID_SERIAL"),
                ]
                .iter()
                .filter_map(|name| device.properties.get(*name).cloned())
                .next();

                Ok(Self {
                    device,
                    path,
                    serial,
                })
            }
            other => Err(SpecializationError::WrongType {
                expected: DeviceType::Disk,
                actual: other.clone(),
            }),
        }
    }
}

impl Deref for Disk {
    type Target = Device;

    fn deref(&self) -> &Self::Target {
        &self.device
    }
}
