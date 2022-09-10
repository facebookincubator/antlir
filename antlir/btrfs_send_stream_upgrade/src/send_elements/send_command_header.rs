/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::mem::size_of_val;
use std::ops::Bound::Excluded;
use std::ops::Bound::Included;

use lazy_static::lazy_static;
use slog::debug;

use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

/*
 * This is taken from the Linux kernel source code.
 * See fs/btrfs/send.h.
 */
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Eq, FromPrimitive, Hash, PartialEq, ToPrimitive)]
#[repr(u16)]
pub enum BtrfsSendCommandType {
    BTRFS_SEND_C_UNSPEC = 0,

    /* Version 1 */
    BTRFS_SEND_C_SUBVOL = 1,
    BTRFS_SEND_C_SNAPSHOT = 2,

    BTRFS_SEND_C_MKFILE = 3,
    BTRFS_SEND_C_MKDIR = 4,
    BTRFS_SEND_C_MKNOD = 5,
    BTRFS_SEND_C_MKFIFO = 6,
    BTRFS_SEND_C_MKSOCK = 7,
    BTRFS_SEND_C_SYMLINK = 8,

    BTRFS_SEND_C_RENAME = 9,
    BTRFS_SEND_C_LINK = 10,
    BTRFS_SEND_C_UNLINK = 11,
    BTRFS_SEND_C_RMDIR = 12,

    BTRFS_SEND_C_SET_XATTR = 13,
    BTRFS_SEND_C_REMOVE_XATTR = 14,

    BTRFS_SEND_C_WRITE = 15,
    BTRFS_SEND_C_CLONE = 16,

    BTRFS_SEND_C_TRUNCATE = 17,
    BTRFS_SEND_C_CHMOD = 18,
    BTRFS_SEND_C_CHOWN = 19,
    BTRFS_SEND_C_UTIMES = 20,

    BTRFS_SEND_C_END = 21,
    BTRFS_SEND_C_UPDATE_EXTENT = 22,
    /*
     * Cannot have duplicates in rust enums
     *
     * BTRFS_SEND_C_MAX_V1 = 22,
     */

    /* Version 2 */
    BTRFS_SEND_C_FALLOCATE = 23,
    BTRFS_SEND_C_SETFLAGS = 24,
    BTRFS_SEND_C_ENCODED_WRITE = 25,
    /* End */
    /*
     * Cannot have duplicates in rust enums
     *
     * BTRFS_SEND_C_MAX_V2 = 25,
     * BTRFS_SEND_C_MAX = 25,
     */
}

lazy_static! {
    static ref COMPRESSIBLE_COMMAND_TYPES: HashMap<BtrfsSendCommandType, (SendVersion, BtrfsSendCommandType)> = hashmap! { BtrfsSendCommandType::BTRFS_SEND_C_WRITE => (SendVersion::SendVersion2, BtrfsSendCommandType::BTRFS_SEND_C_ENCODED_WRITE) };
    static ref UPGRADEABLE_COMMAND_TYPES: HashMap<BtrfsSendCommandType, BTreeSet<SendVersion>> = hashmap! { BtrfsSendCommandType::BTRFS_SEND_C_WRITE => btreeset!{ SendVersion::SendVersion2 } };
    static ref APPENDABLE_COMMAND_TYPES: HashSet<BtrfsSendCommandType> =
        hashset! { BtrfsSendCommandType::BTRFS_SEND_C_WRITE };
    static ref PADDABLE_COMMAND_TYPES: HashSet<BtrfsSendCommandType> =
        hashset! { BtrfsSendCommandType::BTRFS_SEND_C_WRITE };
}

#[derive(Eq, PartialEq)]
pub struct SendCommandHeader {
    /// The size of the following command (excluding the header)
    sch_size: Option<u32>,
    /// The type of the following command
    sch_command_type: BtrfsSendCommandType,
    /// The crc32c of the entire command (other than the crc field)
    sch_crc32c: Option<u32>,
    /// The version of the command header
    sch_version: SendVersion,
}

lazy_static! {
    static ref DUMMY_COMMAND_HEADER: SendCommandHeader = SendCommandHeader {
        sch_size: Some(0),
        sch_command_type: BtrfsSendCommandType::BTRFS_SEND_C_UNSPEC,
        sch_crc32c: Some(0),
        sch_version: SendVersion::SendVersion1
    };
}

impl Display for SendCommandHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let command_type_u16 = num::ToPrimitive::to_u16(&self.sch_command_type).unwrap_or(0xFFu16);
        write!(
            f,
            "<SendCommandHeader Size={:?} CommandType={:#04X} CRC32C={:04X?} Version={}/>",
            self.sch_size, command_type_u16, self.sch_crc32c, self.sch_version
        )
    }
}

impl SendCommandHeader {
    pub fn new(context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        let size = context.read32()?;
        let command_type_u16 = context.read16()?;
        let command_type: BtrfsSendCommandType =
            match num::FromPrimitive::from_u16(command_type_u16) {
                Some(t) => t,
                None => anyhow::bail!(
                    "Constructing a Send Command Header from a Bad Type {}",
                    command_type_u16
                ),
            };
        let crc32c = context.read32()?;
        context.trace_stats();
        let header = SendCommandHeader {
            sch_size: Some(size),
            sch_command_type: command_type,
            sch_crc32c: Some(crc32c),
            sch_version: context.get_source_version()?,
        };
        debug!(context.ssuc_logger, "New CommandHeader={}", header);
        Ok(header)
    }

    pub fn generate_pad_command_header(
        context: &mut SendStreamUpgradeContext,
        size: usize,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        let header = SendCommandHeader {
            sch_size: Some(size as u32),
            sch_command_type: BtrfsSendCommandType::BTRFS_SEND_C_UPDATE_EXTENT,
            sch_crc32c: None,
            sch_version: context.get_destination_version()?,
        };
        debug!(context.ssuc_logger, "Pad CommandHeader={}", header);
        Ok(header)
    }

    pub fn upgrade(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        debug!(
            context.ssuc_logger,
            "Upgrading CommandHeader={} version={}", self, version
        );
        anyhow::ensure!(
            self.is_command_upgradeable(context)?,
            "Trying to upgrade an unupgradeable CommandHeader={}",
            self
        );
        anyhow::ensure!(
            self.sch_size.is_some(),
            "Trying to upgrade a command header with no size CommandHeader={}",
            self
        );
        anyhow::ensure!(
            self.sch_crc32c.is_some(),
            "Trying to upgrade a command header with no crc32c CommandHeader={}",
            self
        );
        // Note that the size and the crc32c won't be fully populated
        // Those will later be updated by the code that clones the command
        let new_header = SendCommandHeader {
            sch_size: None,
            sch_crc32c: None,
            sch_version: version,
            ..*self
        };
        debug!(
            context.ssuc_logger,
            "Upgraded NewCommandHeader={}", new_header
        );
        Ok(new_header)
    }

    pub fn fake_an_upgrade(&mut self, context: &SendStreamUpgradeContext) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.is_command_upgradeable(context)?,
            "Trying to fake upgrade an upgradeable CommandHeader={}",
            self
        );
        self.sch_version = context.get_destination_version()?;
        Ok(())
    }

    pub fn compress(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        debug!(
            context.ssuc_logger,
            "Compressing CommandHeader={} version={}", self, version
        );
        anyhow::ensure!(
            self.is_command_compressible(),
            "Trying to compress an uncompressible CommandHeader={}",
            self
        );
        anyhow::ensure!(
            self.sch_size.is_some(),
            "Trying to compress a command header with no size CommandHeader={}",
            self
        );
        // Note that the size and the crc32c won't be fully populated
        // Those will later be updated by the code that clones the command
        let new_header = SendCommandHeader {
            sch_size: None,
            sch_command_type: self.get_compressed_command_type()?,
            sch_crc32c: None,
            sch_version: version,
        };
        debug!(
            context.ssuc_logger,
            "Compressed NewCommandHeader={}", new_header
        );
        Ok(new_header)
    }

    pub fn copy(
        &self,
        context: &mut SendStreamUpgradeContext,
        command_payload_size: Option<u32>,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        debug!(
            context.ssuc_logger,
            "Copying header from CommandHeader={}", self
        );
        // Typically these commands should have stale sizes & CRC32C values
        anyhow::ensure!(
            self.sch_size.is_some(),
            "Trying to copy size-less CommandHeader={}",
            self
        );
        anyhow::ensure!(
            self.sch_crc32c.is_some(),
            "Trying to copy crc32c-less CommandHeader={}",
            self
        );
        let new_header = SendCommandHeader {
            sch_size: command_payload_size,
            sch_crc32c: Some(0),
            ..*self
        };
        debug!(
            context.ssuc_logger,
            "Copied header CommandHeader={}", new_header
        );
        Ok(new_header)
    }

    pub fn persist(
        &self,
        context: &mut SendStreamUpgradeContext,
        skip_crc32c: bool,
    ) -> anyhow::Result<()> {
        context.trace_stats();
        debug!(context.ssuc_logger, "Writing CommandHeader={}", self);
        let size = match self.sch_size {
            Some(value) => value,
            None => anyhow::bail!("Trying to persist a command header with unpopulated size"),
        };
        let crc32c = if skip_crc32c {
            0
        } else {
            match self.sch_crc32c {
                Some(value) => value,
                None => anyhow::bail!("Trying to persist a command header with unpopulated crc32c"),
            }
        };
        let destination_version = context.get_destination_version()?;
        anyhow::ensure!(
            self.sch_version == destination_version,
            "Version mismatch while persisting struct version={} destination version={}",
            self.sch_version,
            destination_version
        );
        let mut offset = context.get_write_offset();
        context.write32(size)?;
        let command_type_u16 = num::ToPrimitive::to_u16(&self.sch_command_type).unwrap_or(0xFFu16);
        context.write16(command_type_u16)?;
        context.write32(crc32c)?;
        offset = context.get_write_offset() - offset;
        anyhow::ensure!(
            offset == Self::get_header_size(),
            "Wrote {}B out of {}B header",
            offset,
            Self::get_header_size()
        );
        Ok(())
    }

    pub fn set_size(&mut self, size: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.sch_size.is_none(),
            "Can only overwrite size if it is not set"
        );
        self.sch_size = Some(size);
        Ok(())
    }

    pub fn set_crc32c(&mut self, crc32c: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.sch_crc32c.unwrap_or(0) == 0,
            "Can only overwrite crc32c if it is not set or zero"
        );
        self.sch_crc32c = Some(crc32c);
        Ok(())
    }

    fn get_compressed_command_type(&self) -> anyhow::Result<BtrfsSendCommandType> {
        match COMPRESSIBLE_COMMAND_TYPES.get(&self.sch_command_type) {
            Some((version, new_command_type)) => {
                anyhow::ensure!(
                    version <= &self.sch_version,
                    "No compressed command type for CommandHeader={} version={}",
                    self,
                    version
                );
                Ok(*new_command_type)
            }
            None => anyhow::bail!("No key found for CommandHeader={}", self),
        }
    }

    pub fn is_command_compressible(&self) -> bool {
        match COMPRESSIBLE_COMMAND_TYPES.get(&self.sch_command_type) {
            Some((version, _new_command_type)) => version <= &self.sch_version,
            None => false,
        }
    }

    pub fn is_command_upgradeable(
        &self,
        context: &SendStreamUpgradeContext,
    ) -> anyhow::Result<bool> {
        let old_version = self.sch_version;
        let new_version = context.get_destination_version()?;
        // There is nothing to upgrade if the versions match
        if old_version == new_version {
            return Ok(false);
        }
        match UPGRADEABLE_COMMAND_TYPES.get(&self.sch_command_type) {
            // Count the number of versions going from old_version + 1 to new_version
            // If we have at least 1 version where the command type was upgraded, we want
            // to run upgrade processing
            Some(set) => Ok(set
                .range((Excluded(&old_version), Included(&new_version)))
                .count()
                > 0),
            None => Ok(false),
        }
    }

    pub fn is_command_end(&self) -> bool {
        self.sch_command_type == BtrfsSendCommandType::BTRFS_SEND_C_END
    }

    pub fn is_appendable(&self) -> bool {
        APPENDABLE_COMMAND_TYPES.contains(&self.sch_command_type)
    }

    pub fn are_commands_appendable(&self, other: &Self) -> bool {
        self.sch_command_type == other.sch_command_type
            && self.sch_version == other.sch_version
            && self.is_appendable()
    }

    pub fn is_paddable(&self) -> bool {
        PADDABLE_COMMAND_TYPES.contains(&self.sch_command_type)
    }

    pub fn get_header_size() -> usize {
        let size = DUMMY_COMMAND_HEADER.sch_size.unwrap_or(0);
        let command_type = DUMMY_COMMAND_HEADER.sch_command_type;
        let crc32c = DUMMY_COMMAND_HEADER.sch_size.unwrap_or(0);
        // TODO: Remove this temporary check.
        if size_of_val(&command_type) != 2 {
            panic!("Found bad command type size {}", size_of_val(&command_type));
        }
        size_of_val(&size) + size_of_val(&command_type) + size_of_val(&crc32c)
    }

    pub fn get_command_payload_size(&self) -> anyhow::Result<usize> {
        match self.sch_size {
            Some(size) => Ok(size as usize),
            None => anyhow::bail!("Attempting to extract an unitialized size"),
        }
    }

    pub fn get_crc32c(&self) -> anyhow::Result<u32> {
        match self.sch_crc32c {
            Some(crc32c) => Ok(crc32c),
            None => anyhow::bail!("Attempting to extract an unitialized crc32c"),
        }
    }
}
