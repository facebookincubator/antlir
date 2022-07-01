/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use crate::Error;
use crate::Result;
use crate::Subsystem;

mod disk;
mod partition;
pub use disk::Disk;
pub use partition::Partition;

/// Device type within a subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceType {
    /// Standalone disk device (eg nvme0n1)
    Disk,
    /// Partition on a disk device (eg nvme0n1p1)
    Partition,
    /// Device type that is not a first-class citizen of this library
    Other(OsString),
    /// Udev has no knowledge of device type
    None,
}

impl From<&OsStr> for DeviceType {
    fn from(s: &OsStr) -> Self {
        if s == OsStr::new("disk") {
            Self::Disk
        } else if s == OsStr::new("partition") {
            Self::Partition
        } else {
            Self::Other(s.into())
        }
    }
}

/// A Rust-wrapped Device from udev, since udev::Device is not Send. This also
/// enables some nicer accessors of properties.
#[derive(Debug, Clone)]
pub struct Device {
    syspath: PathBuf,
    parent: Option<Box<Device>>,
    subsystem: Subsystem,
    devtype: DeviceType,
    properties: HashMap<OsString, OsString>,
    dev_path: Option<PathBuf>,
}

impl From<&udev::Device> for Device {
    fn from(dev: &udev::Device) -> Self {
        Self {
            syspath: dev.syspath().to_owned(),
            dev_path: dev.devnode().map(PathBuf::from),
            parent: dev.parent().map(|parent| Self::from(&parent)).map(Box::new),
            subsystem: dev.subsystem().map_or(Subsystem::None, Subsystem::from),
            devtype: dev.devtype().map_or(DeviceType::None, DeviceType::from),
            properties: dev
                .properties()
                .map(|ent| (ent.name().to_owned(), ent.value().to_owned()))
                .collect(),
        }
    }
}

impl Device {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let udev_dev = match path.starts_with("/sys") {
            true => udev::Device::from_syspath(path),
            false => {
                let stat = nix::sys::stat::stat(path).map_err(|error| Error::Lookup {
                    path: path.to_path_buf(),
                    error,
                })?;
                let major = nix::sys::stat::major(stat.st_rdev);
                let minor = nix::sys::stat::minor(stat.st_rdev);
                let mode = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
                let is_block = mode.contains(nix::sys::stat::SFlag::S_IFBLK);
                let is_char = mode.contains(nix::sys::stat::SFlag::S_IFCHR);
                // is_block and is_char are NOT mutually exclusive, block
                // devices are also reported as char devices, so we must check
                // is_block first, then is_char
                let sys_path: PathBuf = match (is_block, is_char) {
                    (true, _) => format!("/sys/dev/block/{}:{}", major, minor),
                    (_, false) => format!("/sys/dev/char/{}:{}", major, minor),
                    _ => return Err(Error::NotADevice(path.to_path_buf())),
                }
                .into();
                udev::Device::from_syspath(&sys_path)
            }
        }?;
        Ok(Self::from(&udev_dev))
    }

    pub fn parent(&self) -> Option<&Device> {
        self.parent.as_deref()
    }

    pub fn subsystem(&self) -> &Subsystem {
        &self.subsystem
    }

    pub fn device_type(&self) -> &DeviceType {
        &self.devtype
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.syspath == other.syspath
    }
}

impl<D> PartialEq<D> for Device
where
    D: SpecificDevice,
{
    fn eq(&self, other: &D) -> bool {
        self.syspath == other.syspath
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpecializationError {
    #[error("expected {expected:?}, device was {actual:?}")]
    WrongType {
        expected: DeviceType,
        actual: DeviceType,
    },
    #[error("this device type requires a parent but none was found")]
    MissingParent,
    #[error("{property} was invalid: {error:?}")]
    Property {
        property: &'static str,
        error: PropertyError,
    },
}

#[derive(Debug, Error)]
pub enum PropertyError {
    #[error("required property is missing")]
    Missing,
    #[error("invalid value: {0:?}")]
    InvalidValue(anyhow::Error),
}

pub trait SpecificDevice:
    Deref<Target = Device> + Into<Device> + TryFrom<Device, Error = SpecializationError>
{
    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let dev = Device::from_path(path)?;
        dev.try_into().map_err(Error::Specialization)
    }
}
