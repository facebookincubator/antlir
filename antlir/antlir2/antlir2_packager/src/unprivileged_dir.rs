/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::Permissions;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use walkdir::WalkDir;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnprivilegedDir {
    layer: PathBuf,
}

impl UnprivilegedDir {
    pub(crate) fn build(
        &self,
        out: &Path,
        root_guard: Option<antlir2_rootless::EscalationGuard>,
    ) -> Result<()> {
        let layer = self.layer.canonicalize()?;
        std::fs::create_dir(out).context("while creating root")?;
        std::os::unix::fs::lchown(
            out,
            root_guard
                .as_ref()
                .and_then(|r| r.unprivileged_uid())
                .map(|i| i.as_raw()),
            root_guard
                .as_ref()
                .and_then(|r| r.unprivileged_gid())
                .map(|i| i.as_raw()),
        )
        .context("while chowning root")?;
        for entry in WalkDir::new(&layer) {
            let entry = entry?;
            let relpath = entry.path().strip_prefix(&layer)?;
            if relpath == Path::new("") {
                continue;
            }
            let dst = out.join(relpath);
            if entry.file_type().is_dir() {
                std::fs::create_dir(&dst)
                    .with_context(|| format!("while creating directory '{}'", dst.display()))?;
                std::fs::set_permissions(&dst, Permissions::from_mode(0o755))?;
            } else if entry.file_type().is_symlink() {
                let target = std::fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(target, &dst)
                    .with_context(|| format!("while creating symlink '{}'", dst.display()))?;
            } else if entry.file_type().is_file() {
                std::fs::copy(entry.path(), &dst)
                    .with_context(|| format!("while copying file '{}'", dst.display()))?;
                let mut mode = entry.metadata()?.mode();
                // preserve executable bit
                if (mode & 0o111) != 0 {
                    mode |= 0o111;
                }
                // always allow read
                mode |= 0o444;
                // remove write bits
                mode &= !0o222;
                std::fs::set_permissions(&dst, Permissions::from_mode(mode))?;
            }
            std::os::unix::fs::lchown(
                &dst,
                root_guard
                    .as_ref()
                    .and_then(|r| r.unprivileged_uid())
                    .map(|i| i.as_raw()),
                root_guard
                    .as_ref()
                    .and_then(|r| r.unprivileged_gid())
                    .map(|i| i.as_raw()),
            )
            .with_context(|| format!("while chowning '{}'", dst.display()))?;
        }
        Ok(())
    }
}
