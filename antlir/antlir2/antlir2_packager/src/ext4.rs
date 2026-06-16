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
use crate::pad_to_align;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ext4 {
    build_appliance: BuildAppliance,
    label: Option<String>,
    size_mb: Option<u64>,
    free_mb: u64,
    fixed_metadata: bool,
    #[serde(default)]
    align_bytes: Option<u64>,
}

const MAPPED_OUTPUT: &str = "/__antlir2__/out/ext4";
const BLOCK_SIZE: u64 = 4096;
const INODE_SIZE: u64 = 256;

/// Fixed timestamp for reproducible ext4 images.
/// February 4, 2004 - the initial launch of thefacebook.com.
/// This matches the FIXED_MTIME used by the OCI tar layer builder.
const REPRODUCIBLE_SOURCE_DATE_EPOCH: &str = "1075852800";

/// Deterministic UUID for reproducible ext4 images.
/// Without this, mkfs.ext4 generates a random v4 UUID on every run.
const REPRODUCIBLE_FS_UUID: &str = "00000000-0000-4000-8000-000000000000";

/// Deterministic hash seed for ext4 dir_index.
/// Without this, mkfs.ext4 generates a random hash seed on every run.
const REPRODUCIBLE_HASH_SEED: &str = "00000000-0000-4000-8000-000000000000";

impl PackageFormat for Ext4 {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let mut binding = IsolationContext::builder(self.build_appliance.path());
        let isol_context = binding
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs((MAPPED_OUTPUT, out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"));

        let isol_context = if self.fixed_metadata {
            isol_context
                .setenv(("SOURCE_DATE_EPOCH", REPRODUCIBLE_SOURCE_DATE_EPOCH))
                // For resize2fs, https://fburl.com/hl0iah95
                .setenv(("E2FSPROGS_FAKE_TIME", REPRODUCIBLE_SOURCE_DATE_EPOCH))
                .build()
        } else {
            isol_context.build()
        };

        let isol = unshare(isol_context)?;
        let mut cmd = isol.command("mkfs.ext4")?;

        if self.fixed_metadata {
            // Fix filesystem UUID to prevent random generation each build
            cmd.arg("-U").arg(REPRODUCIBLE_FS_UUID);
        }

        if let Some(label) = &self.label {
            cmd.arg("-L").arg(label);
        }
        cmd.arg("-d").arg("/__antlir2__/root");
        cmd.arg(MAPPED_OUTPUT);
        cmd.arg("-O");
        // Features derived from https://linux.die.net/man/8/mkfs.ext4
        cmd.arg("dir_index,extent,large_file,sparse_super,uninit_bg");
        cmd.arg("-E");
        let extended_options = [
            "discard",
            if self.fixed_metadata {
                "lazy_itable_init=0"
            } else {
                "lazy_itable_init=1"
            },
            if self.fixed_metadata {
                "lazy_journal_init=0"
            } else {
                "lazy_journal_init=1"
            },
        ]
        .into_iter()
        .chain(
            self.fixed_metadata
                .then(|| format!("hash_seed={REPRODUCIBLE_HASH_SEED}"))
                .as_deref(),
        )
        .collect::<Vec<_>>()
        .join(",");
        cmd.arg(extended_options);
        if let Some(size_mb) = self.size_mb {
            cmd.arg(format!("{}M", size_mb));
            run_cmd(&mut cmd).context("failed to build ext4 archive")?;
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
            // We can't really determine the minimal size of an ext4 image file
            // given a directory - we can only approximate it.
            // The "user annoyance factor" of a failed build is *extremely*
            // high, so let's dramatically overestimate (25% more than what we
            // think) the space that we might need, create an ext4 filesystem
            // with that much space, then shrink it down.
            let size = ByteSize::b((total_file_size.0 as f64 * 1.25) as u64);
            let size = std::cmp::max(
                size,
                // ext4 filesystems must be at least 2 MiB
                ByteSize::mib(2),
            );
            // Round up
            // It's just one kilobyte Michael, what could it cost? $10?
            let size_kb = (size.0 / 1024) + 1;
            cmd.arg(format!("{size_kb}K"));
            run_cmd(&mut cmd).context("failed to build ext4 archive")?;

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
                    .context("failed to resize ext4 archive")?;
            }
        };

        // Run e2fsck to clear residual fs state (journal replay, clean
        // unmount flag, etc.) left by mkfs.ext4/resize2fs. Exit code 1
        // means errors were found and corrected, which is the point of
        // running this — accept it as success.
        // Skip when fixed_metadata is on: e2fsck rewrites metadata in
        // ways that depend on e2fsprogs version/timing and would defeat
        // the byte-for-byte reproducibility the caller asked for.
        if !self.fixed_metadata {
            let mut fsck = isol.command("e2fsck")?;
            fsck.arg("-fy").arg(MAPPED_OUTPUT);
            let output = fsck
                .output()
                .with_context(|| format!("failed to run command: {fsck:?}"))?;
            let code = output.status.code().unwrap_or(-1);
            if code != 0 && code != 1 {
                return Err(anyhow::anyhow!(
                    "e2fsck failed ({:?}): {}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
        }

        if let Some(align) = self.align_bytes {
            pad_to_align(out, align).context("while aligning ext4 image")?;
        }

        Ok(())
    }
}
