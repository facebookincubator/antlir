/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use derivative::Derivative;
use nix::ioctl_read;
use nix::ioctl_readwrite;
use nix::ioctl_write_ptr;

const IOCTL_MAGIC: u64 = 0x94;
pub(crate) const FIRST_FREE_OBJECTID: u64 = 256;
const INO_LOOKUP_PATH_MAX: usize = 4080;
pub(crate) const SUBVOL_NAME_MAX: usize = 4039;
const PATH_NAME_MAX: usize = 4087;
pub(crate) const SPEC_BY_ID: u64 = 1 << 3;
const VOL_NAME_MAX: usize = 255;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct ino_lookup_args {
    pub treeid: u64,
    pub objectid: u64,
    pub name: [u8; INO_LOOKUP_PATH_MAX],
}

impl Default for ino_lookup_args {
    fn default() -> Self {
        Self {
            treeid: 0,
            objectid: 0,
            name: [0; INO_LOOKUP_PATH_MAX],
        }
    }
}

ioctl_readwrite!(ino_lookup, IOCTL_MAGIC, 18, ino_lookup_args);

ioctl_read!(get_flags, IOCTL_MAGIC, 25, u64);
ioctl_write_ptr!(set_flags, IOCTL_MAGIC, 26, u64);

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct vol_args_v2 {
    pub fd: u64,
    pub transid: u64,
    pub flags: u64,
    // this is technically a union but we never mess with qgroups anyway
    pub _unused: [u64; 4],
    pub id: vol_args_v2_spec,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub union vol_args_v2_spec {
    pub name: [u8; SUBVOL_NAME_MAX + 1],
    pub devid: u64,
    pub subvolid: u64,
}

impl Default for vol_args_v2_spec {
    fn default() -> Self {
        Self { subvolid: 0 }
    }
}

ioctl_write_ptr!(snap_destroy_v2, IOCTL_MAGIC, 63, vol_args_v2);
ioctl_write_ptr!(snap_create_v2, IOCTL_MAGIC, 23, vol_args_v2);
ioctl_write_ptr!(subvol_create_v2, IOCTL_MAGIC, 24, vol_args_v2);

#[derive(Copy, Clone)]
#[repr(C)]
pub struct vol_args {
    pub fd: u64,
    pub name: [u8; PATH_NAME_MAX + 1],
}

ioctl_write_ptr!(snap_destroy, IOCTL_MAGIC, 15, vol_args);

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct btrfs_ioctl_timespec {
    pub sec: u64,
    pub nsec: u32,
}

#[derive(Copy, Clone, Derivative)]
#[derivative(Default)]
#[repr(C)]
pub struct get_subvol_info_args {
    pub id: u64,
    /// Name of this subvolume, used to get the real name at mount point
    #[derivative(Default(value = "[0; 256]"))]
    pub name: [u8; VOL_NAME_MAX + 1],
    pub parent_id: u64,
    pub dirid: u64,
    pub generation: u64,
    pub flags: u64,
    pub uuid: [u8; 16],
    pub parent_uuid: [u8; 16],
    pub received_uuid: [u8; 16],
    pub ctransid: u64,
    pub otransid: u64,
    pub stransid: u64,
    pub rtransid: u64,
    pub ctime: btrfs_ioctl_timespec,
    pub otime: btrfs_ioctl_timespec,
    pub stime: btrfs_ioctl_timespec,
    pub rtime: btrfs_ioctl_timespec,
    pub reserved: [u64; 8],
}
ioctl_read!(get_subvol_info, IOCTL_MAGIC, 60, get_subvol_info_args);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ino_lookup_args_size() {
        assert_eq!(std::mem::size_of::<ino_lookup_args>(), 4096);
    }

    #[test]
    fn vol_args_v2_size() {
        assert_eq!(std::mem::size_of::<vol_args_v2>(), 4096);
    }

    #[test]
    fn vol_args_size() {
        assert_eq!(std::mem::size_of::<vol_args>(), 4096);
    }
}
