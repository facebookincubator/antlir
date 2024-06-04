/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use tracing::error;

use crate::buck::Layer;
use crate::manifest::Manifest;

/// We use BUCK_SCRATCH_PATH to hold all the scratch data:
/// * re-hydrated parent layers
/// * hydrated upper directory
/// * workdir
/// * mountpoint
///
/// The hydrated upper directory is where all the changes for the current layer
/// are held by overlayfs. After finalizing the layer, this gets dehydrated into
/// a `manifest` and corresponding `data_dir` that gets moved into the final
/// output locations that buck is expecting.
pub struct Scratch {
    root: PathBuf,
    mountpoint: PathBuf,
    upper: PathBuf,
    work: PathBuf,
    lowers: Vec<PathBuf>,
}

impl Scratch {
    pub(crate) fn setup(root: PathBuf, layers: &[Layer]) -> anyhow::Result<Self> {
        create_dir_all(&root)
            .with_context(|| format!("while creating root '{}'", root.display()))?;

        let mountpoint = root.join("mountpoint");
        create_dir_all(&mountpoint)
            .with_context(|| format!("while creating mountpoint '{}'", mountpoint.display()))?;

        let upper = root.join("upper");
        create_dir_all(&upper)
            .with_context(|| format!("while creating upper '{}'", upper.display()))?;

        let work = root.join("work");
        create_dir_all(&work)
            .with_context(|| format!("while creating work '{}'", work.display()))?;

        let lower_base = root.join("lower");
        create_dir_all(&lower_base)
            .with_context(|| format!("while creating lower base dir '{}'", lower_base.display()))?;
        let mut lowers = vec![];
        for (idx, layer) in layers.iter().enumerate() {
            let lower = lower_base.join(idx.to_string());
            crate::data_dir::unmangle(&layer.data_dir, &lower)
                .with_context(|| format!("while unmangling '{}'", layer.data_dir.display()))?;
            let manifest = std::fs::read(&layer.manifest).with_context(|| {
                format!("while reading manifest '{}'", layer.manifest.display())
            })?;
            let manifest: Manifest = serde_json::from_slice(&manifest).with_context(|| {
                format!(
                    "while deserializing manifest '{}'",
                    layer.manifest.display()
                )
            })?;
            manifest.fix_directory(&lower).with_context(|| {
                format!("while fixing metadata in '{}'", layer.data_dir.display())
            })?;
            lowers.push(lower)
        }
        // overlayfs always requires a lower directory, so for consistency just
        // make sure we at least have an empty one
        if lowers.is_empty() {
            let empty = lower_base.join("empty");
            create_dir_all(&empty)
                .with_context(|| format!("while creating empty lowerdir '{}'", empty.display()))?;
            lowers.push(empty);
        }

        Ok(Self {
            root: root.to_owned(),
            mountpoint,
            upper,
            work,
            lowers,
        })
    }

    pub(crate) fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }

    pub(crate) fn workdir(&self) -> &Path {
        &self.work
    }

    pub(crate) fn upperdir(&self) -> &Path {
        &self.upper
    }

    pub(crate) fn lowerdirs(&self) -> impl Iterator<Item = &Path> {
        self.lowers.iter().map(PathBuf::as_path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Attempt to delete all the scratch contents on drop, since buck2 might
        // complain if we leave files owned by other users (root in the
        // namespace, the regular build user in the parent)
        if let Err(e) = remove_dir_all(&self.root) {
            error!("failed to remove scratch dirs: {e}");
        }
    }
}
