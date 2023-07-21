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
pub fn copy_with_metadata(
    src: &Path,
    dst: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)?;
    let uid = uid.unwrap_or(metadata.uid());
    let gid = gid.unwrap_or(metadata.gid());

    trace!("read metadata: {metadata:?}");
    if metadata.is_symlink() {
        let target = std::fs::read_link(src)?;
        std::os::unix::fs::symlink(target, dst)?;
        std::os::unix::fs::lchown(dst, Some(uid), Some(gid))?;
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
    // There are cases where we might have to re-set the {ug}id to a corrected
    // value if the name differs between two layers, but it's reasonably sane to
    // default to using the same uid/gid instead of root:root as would result
    // otherwise if we just copied without chowning
    trace!("setting owner to {}:{}", uid, gid);
    fchown(&f, Some(uid), Some(gid))?;
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
