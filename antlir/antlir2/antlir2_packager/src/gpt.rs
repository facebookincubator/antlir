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

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bytesize::ByteSize;
use serde::Deserialize;
use uuid::Uuid;

use crate::PackageFormat;

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
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Partition {
    src: PathBuf,
    #[serde(rename = "type")]
    partition_type: PartitionType,
    name: Option<String>,
    alignment: Option<u64>,
}

impl PackageFormat for Gpt {
    fn build(&self, out: &Path) -> Result<()> {
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
            .initialized(false)
            .writable(true)
            .logical_block_size(self.block_size.into())
            .create_from_device(Box::new(file), None)
            .context("while creating new gpt")?;

        gdisk
            .update_partitions(Default::default())
            .context("while making blank gpt table")?;

        if let Some(guid) = self.disk_guid {
            gdisk
                .update_guid(Some(
                    guid.to_string().parse().context("while re-parsing uuid")?,
                ))
                .context("while setting guid")?;
        }

        for partition in self.partitions.iter() {
            let src_size = ByteSize::b(partition.src.metadata()?.len());
            let id = gdisk
                .add_partition(
                    partition.name.as_deref().unwrap_or_default(),
                    src_size.as_u64(),
                    partition.partition_type.gpt_type(),
                    0,
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
        }

        gdisk.write().context("while writing partition table")?;

        Ok(())
    }
}
