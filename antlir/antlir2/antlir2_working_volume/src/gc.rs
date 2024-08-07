/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;

use antlir2_btrfs::Subvolume;
use once_cell::sync::Lazy;
use tracing::warn;

use crate::Error;
use crate::Result;
use crate::WorkingVolume;

static AGE_THRESHOLD: Lazy<Duration> = Lazy::new(|| {
    if cfg!(facebook) {
        justknobs::get("antlir2/compiler:gc_if_older_than_sec", None)
            .map(|s| Duration::from_secs(s as u64))
            .unwrap_or(Duration::from_days(14))
    } else {
        Duration::from_days(1)
    }
});

impl WorkingVolume {
    pub fn garbage_collect_old_subvols(&self) -> Result<()> {
        for entry in std::fs::read_dir(self.path()).map_err(Error::GarbageCollect)? {
            let entry = entry.map_err(Error::GarbageCollect)?;
            let meta = entry.metadata().map_err(Error::GarbageCollect)?;
            if meta.ino() != 256 {
                // not a subvol
                continue;
            }
            if let Some(age) = meta.created().ok().and_then(|t| t.elapsed().ok()) {
                if age >= *AGE_THRESHOLD {
                    let path = entry.path();
                    if let Err(e) = try_gc_subvol(&path) {
                        warn!("failed to gc subvol {}: {e}", path.display());
                    }
                }
            }
        }
        Ok(())
    }
}

fn try_gc_subvol(path: &Path) -> Result<()> {
    let subvol = Subvolume::open(path)?;
    subvol.delete().map_err(|(_, err)| err)?;
    Ok(())
}
