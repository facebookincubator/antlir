/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use bitflags::bitflags;
use bytesize::ByteSize;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gpt {
    disk_guid: Option<Uuid>,
    partitions: Vec<Partition>,
    #[serde(default)]
    block_size: BlockSize,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PartitionType {
    Esp,
    Linux,
    LinuxRootX64,
    LinuxRootArm64,
    LinuxRootX64Verity,
    LinuxRootArm64Verity,
}

bitflags! {
    // The bitflags Deserialize implementation expects |-joined all-caps flag
    // names, so that's what we serialize in gpt.bzl
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
    #[serde(transparent)]
    struct PartitionFlags: u64 {
        const GROW_FILE_SYSTEM = 1 << 59;
    }
}

#[derive(Debug, Copy, Clone, Default, Deserialize)]
enum BlockSize {
    #[default]
    #[serde(rename = "512")]
    Lb512,
    #[serde(rename = "4096")]
    Lb4096,
}

impl BlockSize {
    fn as_u64(&self) -> u64 {
        match self {
            Self::Lb512 => 512,
            Self::Lb4096 => 4096,
        }
    }
}

impl From<BlockSize> for gpt::disk::LogicalBlockSize {
    fn from(bs: BlockSize) -> gpt::disk::LogicalBlockSize {
        match bs {
            BlockSize::Lb512 => gpt::disk::LogicalBlockSize::Lb512,
            BlockSize::Lb4096 => gpt::disk::LogicalBlockSize::Lb4096,
        }
    }
}

impl PartitionType {
    fn gpt_type(&self) -> gpt::partition_types::Type {
        match self {
            Self::Esp => gpt::partition_types::EFI,
            Self::Linux => gpt::partition_types::LINUX_FS,
            Self::LinuxRootX64 => gpt::partition_types::LINUX_ROOT_X64,
            Self::LinuxRootArm64 => gpt::partition_types::LINUX_ROOT_ARM_64,
            // systemd Discoverable Partitions root-verity types (SD_GPT_ROOT_*_VERITY)
            Self::LinuxRootX64Verity => gpt::partition_types::Type {
                guid: Uuid::parse_str("2c7357ed-ebd2-46d9-aec1-23d437ec2bf5")
                    .expect("valid verity partition GUID"),
                os: gpt::partition_types::OperatingSystem::Linux,
            },
            Self::LinuxRootArm64Verity => gpt::partition_types::Type {
                guid: Uuid::parse_str("df3300ce-d69f-4c92-978c-9bfb0f38d820")
                    .expect("valid verity partition GUID"),
                os: gpt::partition_types::OperatingSystem::Linux,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Partition {
    src: PathBuf,
    #[serde(rename = "type")]
    partition_type: PartitionType,
    #[serde(default)]
    flags: PartitionFlags,
    name: Option<String>,
    alignment: Option<u64>,
    uuid: Option<Uuid>,
    /// Partitions sharing a `verity_pair` value must share a PARTUUID
    /// (systemd Discoverable Partitions pairs a root partition with its
    /// verity hash partition this way). The pair reuses the uuid of the
    /// first partition in the group when no explicit uuid is given.
    #[serde(default)]
    verity_pair: Option<String>,
}

impl Gpt {
    pub(crate) fn build(&self, out: &Path) -> Result<()> {
        // 2mb of scratch space for gpt headers
        let mut total_size = ByteSize::mb(2);
        for partition in &self.partitions {
            let alignment_bytes = partition.alignment.unwrap_or_default();
            if alignment_bytes % self.block_size.as_u64() != 0 {
                return Err(anyhow!("alignment must be a multiple of block size"));
            }

            let src_size = ByteSize::b(partition.src.metadata()?.len());
            // misalignment can waste up to alignment_bytes bytes per partition
            total_size += src_size + alignment_bytes;
        }

        // round total_size to a multiple of the block size
        let remainder = total_size.as_u64() % self.block_size.as_u64();
        let total_size = match remainder {
            0 => total_size,
            remainder => ByteSize::b(total_size.as_u64() + self.block_size.as_u64() - remainder),
        };

        let mut file = OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .truncate(true)
            .open(out)
            .context("while creating output file")?;
        file.set_len(total_size.as_u64())
            .context("while sizing file")?;

        let mut file_for_writing_contents = OpenOptions::new()
            .write(true)
            .open(out)
            .context("while opening second fd")?;

        // Create a protective MBR at LBA0
        let mbr = gpt::mbr::ProtectiveMBR::with_lb_size(
            u32::try_from((total_size.as_u64() / self.block_size.as_u64()) - 1)
                .context("while converting size to u32")?,
        );
        mbr.overwrite_lba0(&mut file).context("while writing mbr")?;

        let mut gdisk = gpt::GptConfig::default()
            .writable(true)
            .logical_block_size(self.block_size.into())
            .create_from_device(Box::new(file), self.disk_guid)
            .context("while creating new gpt")?;

        // In 4.1.0, there is no clean API to replace the part UUID with a custom one
        // https://github.com/Quyzi/gpt/issues/112#issuecomment-3831215604
        let mut partition_replace_ids = Vec::new();
        let mut verity_pair_uuids: std::collections::HashMap<String, Uuid> =
            std::collections::HashMap::new();

        for partition in self.partitions.iter() {
            let src_size = ByteSize::b(partition.src.metadata()?.len());
            let id = gdisk
                .add_partition(
                    partition.name.as_deref().unwrap_or_default(),
                    src_size.as_u64(),
                    partition.partition_type.gpt_type(),
                    partition.flags.bits(),
                    partition
                        .alignment
                        .map(|alignment| alignment / self.block_size.as_u64()),
                )
                .with_context(|| format!("while adding partition {partition:?}"))?;
            let part = gdisk
                .partitions()
                .get(&id)
                .context("while reading back partition")?;
            let start = part
                .bytes_start(self.block_size.into())
                .context("while computing bytes offset")?;
            file_for_writing_contents
                .seek(SeekFrom::Start(start))
                .context("while seeking to partition start")?;
            let mut src = File::open(&partition.src).context("while opening partition src")?;
            std::io::copy(&mut src, &mut file_for_writing_contents)
                .context("while copying partition contents")?;

            if let Some(uuid) = partition.uuid {
                partition_replace_ids.push((id, uuid));
            }

            if let Some(pair) = &partition.verity_pair {
                // All partitions in a verity pair share a PARTUUID. The
                // explicit uuid above (applied later) wins; otherwise reuse
                // the uuid assigned to the first partition in the pair.
                let pair_uuid = *verity_pair_uuids
                    .entry(pair.clone())
                    .or_insert(partition.uuid.unwrap_or(part.part_guid));
                if pair_uuid != part.part_guid {
                    partition_replace_ids.push((id, pair_uuid));
                }
            }
        }

        for (id, uuid) in partition_replace_ids {
            let mut parts = gdisk.take_partitions();
            let part = parts
                .get_mut(&id)
                .with_context(|| format!("Could not get partition {id:?} to modify"))?;
            part.part_guid = uuid;
            gdisk
                .update_partitions(parts)
                .with_context(|| format!("updating part uuid for partition {id:?}"))?;
        }

        gdisk.write().context("while writing partition table")?;

        Ok(())
    }
}
