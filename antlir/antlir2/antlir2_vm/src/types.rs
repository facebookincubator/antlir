/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This file contains data structure that mirrors what described in vm bzl files
//! so that we can directly deserialize a json into Rust structs.

#![allow(dead_code)]
use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use thiserror::Error;

/// Captures property of the disk specified by user to describe a writable disk
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct QCow2DiskOpts {
    /// Path to the base image file
    pub(crate) base_image: Option<String>,
    /// Resize the disk to provide additional space. This will also be size of entire
    /// disk if `base_image` was not given.
    pub(crate) additional_mib: Option<usize>,
    /// Corresponds to driver for -device qemu arg. For disks, this is the interface.
    /// Examples: virtio-blk, nvme
    pub(crate) interface: String,
    /// Physical block size of the disk
    pub(crate) physical_block_size: usize,
    /// Logical block size of the disk
    pub(crate) logical_block_size: usize,
}

/// Operational specific parameters for VM but not related to VM configuration itself
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VMArgs {
    /// Timeout before VM will be terminated. None disables the timeout, which
    /// should only be used for interactive shells for development.
    pub(crate) timeout_s: Option<u32>,
}

/// Everything we need to create and run the VM
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VMOpts {
    /// number of cores
    pub(crate) cpus: usize,
    /// memory size in MiB
    pub(crate) mem_mib: usize,
    /// List of writable disks. We expect at least one disk and the first one
    /// would be the root disk.
    pub(crate) disks: Vec<QCow2DiskOpts>,
    /// Operational specific parameters
    pub(crate) args: VMArgs,
}

/// Location of various binary and data we need to operate the VM
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct RuntimeOpts {
    pub(crate) qemu_system: String,
    pub(crate) qemu_img: String,
    pub(crate) firmware: String,
    pub(crate) roms_dir: String,
}

#[derive(Debug, Error)]
pub enum VMOptsError {
    #[error(transparent)]
    FileError(#[from] std::io::Error),
    #[error(transparent)]
    JsonParsingError(#[from] serde_json::Error),
}

pub(crate) fn parse_opts<T>(filename: &str) -> Result<T, VMOptsError>
where
    T: DeserializeOwned,
{
    let content = fs::read_to_string(Path::new(filename))?;
    Ok(serde_json::from_str(&content)?)
}
