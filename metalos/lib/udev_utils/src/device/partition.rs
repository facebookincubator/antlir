/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::device::{Device, DeviceType, Disk, PropertyError, SpecializationError, SpecificDevice};

#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    device: Device,
    path: PathBuf,
    parent_disk: Disk,
    partnum: u32,
}

impl Partition {
    /// Path in /dev
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The disk that the partition belongs to
    pub fn disk(&self) -> &Disk {
        &self.parent_disk
    }

    /// Integer partition number on the disk
    pub fn number(&self) -> u32 {
        self.partnum
    }
}

impl SpecificDevice for Partition {}

impl From<Partition> for Device {
    fn from(p: Partition) -> Self {
        p.device
    }
}

impl Deref for Partition {
    type Target = Device;

    fn deref(&self) -> &Self::Target {
        &self.device
    }
}

impl TryFrom<Device> for Partition {
    type Error = SpecializationError;

    fn try_from(device: Device) -> std::result::Result<Self, SpecializationError> {
        match device.device_type() {
            DeviceType::Partition => {
                let path = device
                    .dev_path
                    .clone()
                    .ok_or(SpecializationError::Property {
                        property: "devnode",
                        error: PropertyError::Missing,
                    })?;

                let parent_disk = device
                    .parent()
                    .ok_or(SpecializationError::MissingParent)?
                    .clone()
                    .try_into()?;
                let partnum_osstr = device.properties.get(OsStr::new("PARTN")).ok_or(
                    SpecializationError::Property {
                        property: "PARTN",
                        error: PropertyError::Missing,
                    },
                )?;
                let partnum_str =
                    partnum_osstr
                        .to_str()
                        .ok_or_else(|| SpecializationError::Property {
                            property: "PARTN",
                            error: PropertyError::InvalidValue(anyhow!(
                                "{:?} is not utf-8",
                                partnum_osstr
                            )),
                        })?;
                let partnum = partnum_str
                    .parse()
                    .map_err(|_| SpecializationError::Property {
                        property: "PARTN",
                        error: PropertyError::InvalidValue(anyhow!(
                            "{} is not an integer",
                            partnum_str
                        )),
                    })?;
                Ok(Self {
                    path,
                    device,
                    parent_disk,
                    partnum,
                })
            }
            other => Err(SpecializationError::WrongType {
                expected: DeviceType::Partition,
                actual: other.clone(),
            }),
        }
    }
}
