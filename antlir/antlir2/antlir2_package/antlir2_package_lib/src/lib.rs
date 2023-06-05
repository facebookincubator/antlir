/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bytesize::ByteSize;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BtrfsSubvol {
    pub layer: PathBuf,
    pub writable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BtrfsSpec {
    pub subvols: BTreeMap<PathBuf, BtrfsSubvol>,
    pub default_subvol: PathBuf,
    pub compression_level: i32,
    pub label: Option<String>,
    pub free_mb: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec {
    #[serde(rename = "btrfs")]
    Btrfs {
        btrfs_packager_path: Vec<PathBuf>,
        spec: BtrfsSpec,
    },
    #[serde(rename = "sendstream.v2")]
    SendstreamV2 {
        layer: PathBuf,
        compression_level: i32,
    },
    #[serde(rename = "sendstream.zst")]
    SendstreamZst {
        layer: PathBuf,
        compression_level: i32,
    },
    #[serde(rename = "sendstream")]
    Sendstream { layer: PathBuf },

    #[serde(rename = "vfat")]
    Vfat {
        build_appliance: PathBuf,
        layer: PathBuf,
        fat_size: Option<u16>,
        label: Option<String>,
        size_mb: u64,
    },
    #[serde(rename = "cpio.gz")]
    CpioGZ {
        build_appliance: PathBuf,
        layer: PathBuf,
        compression_level: i32,
    },
    #[serde(rename = "cpio.zst")]
    CpioZst {
        build_appliance: PathBuf,
        layer: PathBuf,
        compression_level: i32,
    },
    #[serde(rename = "rpm")]
    Rpm {
        build_appliance: PathBuf,
        layer: PathBuf,
        name: String,
        epoch: i32,
        version: String,
        release: String,
        arch: String,
        license: String,
        summary: String,
        requires: Vec<String>,
        recommends: Vec<String>,
        provides: Vec<String>,
        empty: bool,
    },
    #[serde(rename = "squashfs")]
    SquashFs {
        build_appliance: PathBuf,
        layer: PathBuf,
    },
}

pub fn run_cmd(command: &mut Command) -> Result<std::process::Output> {
    let output = command.output().context("Failed to run command")?;

    match output.status.success() {
        true => Ok(output),
        false => Err(anyhow!("failed to run command {:?}: {:?}", command, output)),
    }
}

pub fn create_empty_file(output: &Path, size: ByteSize) -> Result<()> {
    let mut file = File::create(output).context("failed to create output file")?;
    file.seek(SeekFrom::Start(size.0))
        .context("failed to seek output to specified size")?;
    file.write_all(&[0])
        .context("Failed to write dummy byte at end of file")?;
    file.sync_all()
        .context("Failed to sync output file to disk")?;

    Ok(())
}
