/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fs::File;
use std::fs::FileTimes;
use std::os::unix::fs::fchown;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tracing::trace;
use tracing::warn;
use xattr::FileExt;

use crate::Result;

#[tracing::instrument(ret, err)]
pub(crate) fn copy_with_metadata(src: &Path, dst: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)?;
    trace!("read metadata: {metadata:?}");
    if metadata.is_symlink() {
        let target = std::fs::read_link(src)?;
        std::os::unix::fs::symlink(target, dst)?;
        std::os::unix::fs::lchown(dst, Some(metadata.uid()), Some(metadata.gid()))?;
        return Ok(());
    } else if metadata.is_file() {
        trace!("copying simple file");
        std::fs::copy(src, dst)?;
    } else if metadata.is_dir() {
        trace!("creating new directory");
        std::fs::create_dir(dst)?;
    } else {
        return Err(anyhow::anyhow!(
            "not sure what to do with a non directory/file/symlink: {}",
            src.display()
        )
        .into());
    }
    trace!("opening dst for metadata operations");
    let f = std::fs::File::open(dst)?;
    trace!("setting permissions");
    f.set_permissions(metadata.permissions())?;
    // TODO(T145931158): this is actually dangerous if the file is not owned by
    // root:root, because there is no guarantee that uid X maps to the same
    // username Y in two different images (even if X and Y both exist in both
    // images!). However, antlir1 appears to completely ignore this, and we can
    // reasonably assume that existing use cases for non-root-owned cloned files
    // are for well-known uids that will match in each image, so let's punt for
    // now and solve it later.
    trace!("setting owner to {}:{}", metadata.uid(), metadata.gid());
    fchown(&f, Some(metadata.uid()), Some(metadata.gid()))?;
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    trace!("setting time to {times:?}");
    if let Err(e) = f.set_times(times) {
        warn!("failed to set file times: {e:?}")
    }
    copy_xattrs(src, &f)?;
    Ok(())
}

#[tracing::instrument(skip_all, ret, err)]
pub(crate) fn copy_xattrs(src: &Path, dst: &File) -> Result<()> {
    match xattr::list(src) {
        Ok(names) => {
            let xattrs: HashMap<_, _> = names
                .into_iter()
                .map(|key| xattr::get(src, &key).map(|val| (key, val)))
                .collect::<std::io::Result<_>>()?;
            trace!("xattrs = {xattrs:?}");
            for (key, val) in xattrs {
                if let Some(val) = val {
                    dst.set_xattr(key, &val)?;
                }
            }
            Ok(())
        }
        Err(e) => {
            warn!("could not list xattrs, assuming there are none to copy: {e:?}");
            Ok(())
        }
    }
}
