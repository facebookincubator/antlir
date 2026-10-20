/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use antlir2_path::PathExt;
use antlir2_working_volume::WorkingFormat;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;
use tempfile::TempDir;

use crate::BuildAppliance;
use crate::materialize_cad_stack;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Btrfs {
    build_appliance: BuildAppliance,
    compression_level: i32,
    free_mb: Option<u64>,
    size_mb: Option<u64>,
    label: Option<String>,
    seed_device: bool,
    default_subvol: Option<PathBuf>,
    subvols: BTreeMap<PathBuf, Subvol>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subvol {
    layers: Vec<PathBuf>,
    writable: bool,
}

impl Btrfs {
    pub fn build(&self, out: &Path, working_format: WorkingFormat) -> Result<()> {
        if self.free_mb.is_some() && self.size_mb.is_some() {
            return Err(anyhow!("free_mb and size_mb are mutually exclusive"));
        }
        // use antlir2_isolate to setup a rootdir view of all the subvolumes we
        // want so that mkfs.btrfs can see the desired structure
        let mut common_isol = IsolationContext::builder(self.build_appliance.path());
        common_isol
            .ephemeral(false)
            .readonly()
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/image.btrfs", out));
        #[cfg(facebook)]
        common_isol.platform(["/usr/local/fbcode"]);

        let mut isol = common_isol.clone();
        isol.tmpfs(Path::new("/__antlir2__/root"));

        let mut materialized_roots = Vec::new();
        for (path, subvol) in &self.subvols {
            let root = match working_format {
                WorkingFormat::Btrfs => {
                    if subvol.layers.len() != 1 {
                        return Err(anyhow!("btrfs requires exactly one layer"));
                    }
                    subvol.layers[0].clone()
                }
                WorkingFormat::CadStack => {
                    let dir = TempDir::new().context("while creating materialization directory")?;
                    materialize_cad_stack(&subvol.layers, dir.path())?;
                    let root = dir.path().to_path_buf();
                    materialized_roots.push(dir);
                    root
                }
            };
            isol.inputs((Path::new("/__antlir2__/root").join_abs(path), root));
        }

        let isol_context = isol.build();

        let mut mkfs = unshare(isol_context.clone())?.command("mkfs.btrfs")?;

        if self.compression_level != 0 {
            mkfs.arg("--compress")
                .arg(format!("zstd:{}", self.compression_level));
        } else {
            mkfs.arg("--reflink");
        }
        if let Some(label) = &self.label {
            mkfs.arg("--label").arg(label);
        }
        mkfs.arg("--rootdir").arg("/__antlir2__/root");
        if let Some(size_mb) = self.size_mb {
            // mkfs.btrfs keeps an existing image file's size when it is not
            // asked to shrink.
            std::fs::OpenOptions::new()
                .write(true)
                .open(out)
                .context("while opening image file")?
                .set_len(size_mb * 1024 * 1024)
                .context("while sizing image file")?;
        } else {
            mkfs.arg("--shrink");
            if let Some(free_mb) = self.free_mb {
                mkfs.arg("--shrink-slack-size")
                    .arg((free_mb * 1024 * 1024).to_string());
            }
        }
        for (path, subvol) in &self.subvols {
            let mut subvol_arg = match (subvol.writable, self.default_subvol.as_ref() == Some(path))
            {
                (true, false) => OsString::from("rw:"),
                (true, true) => OsString::from("default:"),
                (false, false) => OsString::from("ro:"),
                (false, true) => OsString::from("default-ro:"),
            };
            subvol_arg.push(path.as_os_str());
            mkfs.arg("--subvol").arg(subvol_arg);
        }

        run_cmd(mkfs.arg("/__antlir2__/out/image.btrfs")).context("while running mkfs.btrfs")?;

        if let Some(size_mb) = self.size_mb {
            // mkfs.btrfs silently grows an image file that it estimates is too
            // small for the rootdir instead of failing.
            let actual = std::fs::metadata(out)
                .context("while checking image size")?
                .len();
            if actual != size_mb * 1024 * 1024 {
                return Err(anyhow!(
                    "size_mb = {size_mb} is too small: mkfs.btrfs needs {} MiB for these contents",
                    actual.div_ceil(1024 * 1024)
                ));
            }
        }

        if self.seed_device {
            let mut isol = common_isol.clone();
            run_cmd(
                unshare(isol.build())?
                    .command("btrfstune")?
                    .arg("-S")
                    .arg("1")
                    .arg("/__antlir2__/out/image.btrfs"),
            )
            .context("while running btrfs tune")?;
        }

        Ok(())
    }
}
