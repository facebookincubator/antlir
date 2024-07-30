/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::mem::size_of_val;

use lazy_static::lazy_static;
use slog::debug;
use slog::trace;

use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

/*
 * This is taken from the Linux kernel source code.
 * See fs/btrfs/send.h.
 */
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Eq, FromPrimitive, Hash, PartialEq, ToPrimitive)]
#[repr(u16)]
pub enum BtrfsSendAttributeType {
    BTRFS_SEND_A_UNSPEC = 0,

    /* Version 1 */
    BTRFS_SEND_A_UUID = 1,
    BTRFS_SEND_A_CTRANSID = 2,

    BTRFS_SEND_A_INO = 3,
    BTRFS_SEND_A_SIZE = 4,
    BTRFS_SEND_A_MODE = 5,
    BTRFS_SEND_A_UID = 6,
    BTRFS_SEND_A_GID = 7,
    BTRFS_SEND_A_RDEV = 8,
    BTRFS_SEND_A_CTIME = 9,
    BTRFS_SEND_A_MTIME = 10,
    BTRFS_SEND_A_ATIME = 11,
    BTRFS_SEND_A_OTIME = 12,

    BTRFS_SEND_A_XATTR_NAME = 13,
    BTRFS_SEND_A_XATTR_DATA = 14,

    BTRFS_SEND_A_PATH = 15,
    BTRFS_SEND_A_PATH_TO = 16,
    BTRFS_SEND_A_PATH_LINK = 17,

    BTRFS_SEND_A_FILE_OFFSET = 18,
    /*
     * As of send stream v2, this attribute is special: it must be the last
     * attribute in a command, its header contains only the type, and its
     * length is implicitly the remaining length of the command.
     */
    BTRFS_SEND_A_DATA = 19,

    BTRFS_SEND_A_CLONE_UUID = 20,
    BTRFS_SEND_A_CLONE_CTRANSID = 21,
    BTRFS_SEND_A_CLONE_PATH = 22,
    BTRFS_SEND_A_CLONE_OFFSET = 23,
    BTRFS_SEND_A_CLONE_LEN = 24,

    /*
     * Cannot have duplicates in rust enums
     *
     * BTRFS_SEND_A_MAX_V1 = 24,
     */

    /* Version 2 */
    BTRFS_SEND_A_FALLOCATE_MODE = 25,

    BTRFS_SEND_A_SETFLAGS_FLAGS = 26,

    BTRFS_SEND_A_UNENCODED_FILE_LEN = 27,
    BTRFS_SEND_A_UNENCODED_LEN = 28,
    BTRFS_SEND_A_UNENCODED_OFFSET = 29,
    /*
     * COMPRESSION and ENCRYPTION default to NONE (0) if omitted from
     * BTRFS_SEND_C_ENCODED_WRITE.
     */
    BTRFS_SEND_A_COMPRESSION = 30,
    BTRFS_SEND_A_ENCRYPTION = 31,
    /* End */
    /*
     * Cannot have duplciates in rust enums
     *
     * BTRFS_SEND_A_MAX_V2 = 31,
     * BTRFS_SEND_A_MAX = 31,
     */
}

pub const BTRFS_ENCODED_IO_COMPRESSION_ZSTD: u32 = 0x2;

lazy_static! {
    // As of v2, SEND_A_DATA can be compressed to SEND_A_DATA
    static ref COMPRESSIBLE_ATTRIBUTE_TYPES: HashMap<BtrfsSendAttributeType, (SendVersion, BtrfsSendAttributeType)> = hashmap!{ BtrfsSendAttributeType::BTRFS_SEND_A_DATA => (SendVersion::SendVersion2, BtrfsSendAttributeType::BTRFS_SEND_A_DATA) };
    // SEND_A_DATA doesn't support a size in v2
    static ref SIZE_LESS_ATTRIBUTE_TYPES: HashMap<BtrfsSendAttributeType, SendVersion> = hashmap!{ BtrfsSendAttributeType::BTRFS_SEND_A_DATA => SendVersion::SendVersion2 };
    // SEND_A_DATA requests can be appended
    static ref APPENDABLE_ATTRIBUTE_TYPES: HashSet<BtrfsSendAttributeType> = hashset!{ BtrfsSendAttributeType::BTRFS_SEND_A_DATA };
}

#[derive(Eq, PartialEq)]
pub struct SendAttributeHeader {
    /// The type of the current attribute
    sah_attribute_type: BtrfsSendAttributeType,
    /// The size of the current attribute (excluding the header)
    sah_size: Option<u16>,
    /// The version of the attribute header
    sah_version: SendVersion,
}

impl Display for SendAttributeHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let attribute_type_u16 =
            num::ToPrimitive::to_u16(&self.sah_attribute_type).unwrap_or(0xFFu16);
        write!(
            f,
            "<SendAttributeHeader AttributeType={:#04X} Size={:?} HeaderSize={} Version={}/>",
            attribute_type_u16,
            self.sah_size,
            self.get_header_size(),
            self.sah_version
        )
    }
}

impl SendAttributeHeader {
    pub fn new(context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_source_version()?;
        let attribute_type_u16 = context.read16()?;
        let attribute_type: BtrfsSendAttributeType =
            match num::FromPrimitive::from_u16(attribute_type_u16) {
                Some(t) => t,
                None => anyhow::bail!(
                    "Constructing a Send Attribute Header from a Bad Type {}",
                    attribute_type_u16
                ),
            };
        // Check to see if the version of the command we're reading supports having a size
        let size: Option<u16> = match SIZE_LESS_ATTRIBUTE_TYPES.get(&attribute_type) {
            Some(size_less_version) => {
                if size_less_version <= &version {
                    None
                } else {
                    Some(context.read16()?)
                }
            }
            None => Some(context.read16()?),
        };
        let header = SendAttributeHeader {
            sah_attribute_type: attribute_type,
            sah_size: size,
            sah_version: version,
        };
        debug!(context.ssuc_logger, "New AttributeHeader={}", header);
        Ok(header)
    }

    pub fn construct(
        attribute_type: BtrfsSendAttributeType,
        size: u16,
        version: SendVersion,
    ) -> Self {
        Self {
            sah_attribute_type: attribute_type,
            sah_size: Some(size),
            sah_version: version,
        }
    }

    pub fn copy(&self) -> Self {
        Self {
            sah_attribute_type: self.sah_attribute_type,
            sah_size: self.sah_size,
            sah_version: self.sah_version,
        }
    }

    pub fn upgrade(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        debug!(context.ssuc_logger, "Upgrading AttributeHeader={}", self);
        anyhow::ensure!(
            self.is_attribute_upgradeable(context)?,
            "Trying to upgrade based on an unupgradeable AttributeHeader={}",
            self
        );
        let source_version = context.get_source_version()?;
        let destination_version = context.get_destination_version()?;
        let new_header = if source_version == SendVersion::SendVersion1
            && destination_version == SendVersion::SendVersion2
        {
            SendAttributeHeader {
                sah_size: None,
                sah_version: destination_version,
                ..*self
            }
        } else {
            SendAttributeHeader { ..*self }
        };
        debug!(
            context.ssuc_logger,
            "Upgraded NewAttributeHeader={}", new_header
        );
        Ok(new_header)
    }

    pub fn compress(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        debug!(context.ssuc_logger, "Upgrading AttributeHeader={}", self);
        anyhow::ensure!(
            self.is_attribute_compressible(),
            "Trying to compress based on an uncompressible AttributeHeader={}",
            self
        );
        let destination_version = context.get_destination_version()?;
        let new_header = SendAttributeHeader {
            sah_attribute_type: self.get_compressed_attribute_type()?,
            sah_size: None,
            sah_version: destination_version,
        };
        debug!(
            context.ssuc_logger,
            "Compressed NewAttributeHeader={}", new_header
        );
        Ok(new_header)
    }

    pub fn persist(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        context.trace_stats();
        debug!(context.ssuc_logger, "Writing AttributeHeader={}", self);
        let destination_version = context.get_destination_version()?;
        anyhow::ensure!(
            self.sah_version == destination_version,
            "Version mismatch while persisting struct version={} destination version={}",
            self.sah_version,
            destination_version
        );
        let attribute_type_u16 = match num::ToPrimitive::to_u16(&self.sah_attribute_type) {
            Some(value) => value,
            None => anyhow::bail!(
                "Failed to convert attribute type {:?} to u16",
                self.sah_attribute_type
            ),
        };
        context.write16(attribute_type_u16)?;
        match self.sah_size {
            Some(size) => context.write16(size)?,
            None => trace!(
                context.ssuc_logger,
                "Size not found while persisting attribute type {:?}", self.sah_attribute_type
            ),
        }
        Ok(())
    }

    pub fn get_compressed_attribute_type(&self) -> anyhow::Result<BtrfsSendAttributeType> {
        match COMPRESSIBLE_ATTRIBUTE_TYPES.get(&self.sah_attribute_type) {
            Some((version, new_attribute_type)) => {
                anyhow::ensure!(
                    version <= &self.sah_version,
                    "No compressed attribute type for AttributeHeader={} version={}",
                    self,
                    version
                );
                Ok(*new_attribute_type)
            }
            None => anyhow::bail!("No key found for AttributeHeader={}", self),
        }
    }

    pub fn is_attribute_compressible(&self) -> bool {
        match COMPRESSIBLE_ATTRIBUTE_TYPES.get(&self.sah_attribute_type) {
            Some((version, _new_attribute_type)) => version <= &self.sah_version,
            None => false,
        }
    }

    pub fn is_attribute_upgradeable(
        &self,
        context: &SendStreamUpgradeContext,
    ) -> anyhow::Result<bool> {
        let old_version = self.sah_version;
        let new_version = context.get_destination_version()?;
        // There is nothing to upgrade if the versions match
        if old_version == new_version {
            return Ok(false);
        }
        match SIZE_LESS_ATTRIBUTE_TYPES.get(&self.sah_attribute_type) {
            // Return if the old version was not sizeless but the new version is;
            // upgrading will result in the size being removed
            Some(version) => Ok(old_version < *version && *version == new_version),
            None => Ok(false),
        }
    }

    pub fn is_attribute_appendable(&self) -> bool {
        APPENDABLE_ATTRIBUTE_TYPES.contains(&self.sah_attribute_type)
    }

    pub fn can_append_attributes(&self, other: &Self) -> bool {
        self.sah_attribute_type == other.sah_attribute_type && self.is_attribute_appendable()
    }

    pub fn is_attribute_truncatable(&self) -> bool {
        // All appendable commands are truncatable too
        self.is_attribute_appendable()
    }

    pub fn has_size(&self) -> bool {
        self.sah_size.is_some()
    }

    pub fn get_attribute_payload_size(&self) -> anyhow::Result<usize> {
        match self.sah_size {
            Some(size) => Ok(size as usize),
            None => anyhow::bail!("Attempting to extract an unitialized size"),
        }
    }

    pub fn get_header_size(&self) -> usize {
        let attribute_type = self.sah_attribute_type;
        let mut header_size = size_of_val(&attribute_type);
        // TODO: Remove this temporary check.
        if header_size != 2 {
            panic!("Found bad attribute type size {}", header_size);
        }
        if let Some(size) = self.sah_size {
            header_size += size_of_val(&size);
        }
        header_size
    }

    pub fn get_attribute_total_size(
        &self,
        context: &mut SendStreamUpgradeContext,
        bytes_remaining: usize,
    ) -> anyhow::Result<usize> {
        let header_size = self.get_header_size();
        match self.get_attribute_payload_size() {
            // The total attribute size is the header size plus the
            // attribute payload size
            Ok(payload_size) => Ok(header_size + payload_size),
            // In the case that the size of the attribute wasn't
            // specified, we need to read all of the bytes remaining
            // NOTE: This can legitimately occur in the read path if v2
            // uncompressed streams are subsequently compressed
            Err(_error) => {
                anyhow::ensure!(
                    bytes_remaining >= header_size,
                    "Only {}B left in source; cannot accommodate header of {}B",
                    bytes_remaining,
                    header_size
                );
                debug!(
                    context.ssuc_logger,
                    "No size found for attribute with header {}", self
                );
                Ok(bytes_remaining)
            }
        }
    }

    pub fn is_attribute_send_a_path(&self) -> bool {
        self.sah_attribute_type == BtrfsSendAttributeType::BTRFS_SEND_A_PATH
    }

    pub fn is_attribute_send_a_file_offset(&self) -> bool {
        self.sah_attribute_type == BtrfsSendAttributeType::BTRFS_SEND_A_FILE_OFFSET
    }

    pub fn is_attribute_send_a_data(&self) -> bool {
        self.sah_attribute_type == BtrfsSendAttributeType::BTRFS_SEND_A_DATA
    }
}
