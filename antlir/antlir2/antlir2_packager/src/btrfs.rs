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

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use antlir2_path::PathExt;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::run_cmd;
use crate::BuildAppliance;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Btrfs {
    build_appliance: BuildAppliance,
    compression_level: i32,
    free_mb: Option<u64>,
    label: Option<String>,
    seed_device: bool,
    default_subvol: Option<PathBuf>,
    subvols: BTreeMap<PathBuf, Subvol>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subvol {
    layer: PathBuf,
    writable: bool,
}

impl Btrfs {
    pub fn build(&self, out: &Path) -> Result<()> {
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

        for (path, subvol) in &self.subvols {
            isol.inputs((
                Path::new("/__antlir2__/root").join_abs(path),
                subvol.layer.clone(),
            ));
        }

        let isol_context = isol.build();

        let mut mkfs = unshare(isol_context.clone())?.command("mkfs.btrfs")?;

        mkfs.arg("--compress")
            .arg(format!("zstd:{}", self.compression_level));
        if let Some(label) = &self.label {
            mkfs.arg("--label").arg(label);
        }
        mkfs.arg("--rootdir").arg("/__antlir2__/root");
        mkfs.arg("--shrink");
        if let Some(free_mb) = self.free_mb {
            mkfs.arg("--shrink-slack-size")
                .arg((free_mb * 1024 * 1024).to_string());
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
