/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec {
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
    #[serde(rename = "vfat")]
    Vfat {
        layer: PathBuf,
        fat_size: Option<u16>,
        label: Option<String>,
        size_mb: u64,
    },
    #[serde(rename = "cpio.gz")]
    CpioGZ {
        layer: PathBuf,
        compression_level: i32,
    },
    #[serde(rename = "cpio.zst")]
    CpioZst {
        layer: PathBuf,
        compression_level: i32,
    },
}

pub fn run_cmd(command: &mut Command) -> Result<std::process::Output> {
    let output = command.output().context("Failed to run command")?;

    match output.status.success() {
        true => Ok(output),
        false => Err(anyhow!("failed to run command {:?}: {:?}", command, output)),
    }
}
