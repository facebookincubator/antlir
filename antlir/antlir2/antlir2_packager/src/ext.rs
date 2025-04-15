/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use bytesize::ByteSize;
use serde::Deserialize;
use walkdir::WalkDir;

use crate::BuildAppliance;
use crate::PackageFormat;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ext3 {
    build_appliance: BuildAppliance,
    label: Option<String>,
    size_mb: Option<u64>,
    free_mb: u64,
}

const MAPPED_OUTPUT: &str = "/__antlir2__/out/ext3";
const BLOCK_SIZE: u64 = 4096;
const INODE_SIZE: u64 = 256;

impl PackageFormat for Ext3 {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs((MAPPED_OUTPUT, out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        let isol = unshare(isol_context)?;
        let mut cmd = isol.command("mkfs.ext3")?;
        if let Some(label) = &self.label {
            cmd.arg("-L").arg(label);
        }
        cmd.arg("-d").arg("/__antlir2__/root");
        cmd.arg(MAPPED_OUTPUT);
        if let Some(size_mb) = self.size_mb {
            cmd.arg(format!("{}M", size_mb));
            run_cmd(&mut cmd).context("failed to build ext3 archive")?;
        } else {
            let total_file_size = ByteSize::b(
                WalkDir::new(layer)
                    .into_iter()
                    .map(|entry| {
                        entry.context("while walking directory").and_then(|e| {
                            let size = e.metadata().map(|m| m.len()).with_context(|| {
                                format!("while getting size of {}", e.path().display())
                            })?;
                            if size < 60 {
                                // small files can be stored entirely in the inode
                                Ok(INODE_SIZE)
                            } else {
                                // otherwise a file makes up some number of
                                // blocks, plus an inode
                                Ok((size.div_ceil(BLOCK_SIZE) * BLOCK_SIZE) + INODE_SIZE)
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .sum(),
            );
            // Well this is kinda crazy... Here goes:
            // We can't really determine the minimal size of an ext3 image file
            // given a directory - we can only approximate it.
            // The "user annoyance factor" of a failed build is *extremely*
            // high, so let's dramatically overestimate (25% more than what we
            // think) the space that we might need, create an ext3 filesystem
            // with that much space, then shrink it down.
            let size = ByteSize::b((total_file_size.0 as f64 * 1.25) as u64);
            let size = std::cmp::max(
                size,
                // ext3 filesystems must be at least 2 MiB
                ByteSize::mib(2),
            );
            // Round up
            // It's just one kilobyte Michael, what could it cost? $10?
            let size_kb = (size.0 / 1024) + 1;
            cmd.arg(format!("{size_kb}K"));
            run_cmd(&mut cmd).context("failed to build ext3 archive")?;

            run_cmd(isol.command("resize2fs")?.arg("-M").arg(MAPPED_OUTPUT))
                .context("while minimizing fs size")?;

            // Now, if the user asked for some free space, we need to give it to
            // them.
            if self.free_mb != 0 {
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(out)
                    .context("while opening image file")?;
                let size = f.metadata().context("while getting image size")?.len();

                let new_size = size + ByteSize::mib(self.free_mb);
                f.set_len(new_size.0).context("while growing image file")?;
                run_cmd(isol.command("resize2fs")?.arg(MAPPED_OUTPUT))
                    .context("failed to resize ext3 archive")?;
            }
        };

        Ok(())
    }
}
