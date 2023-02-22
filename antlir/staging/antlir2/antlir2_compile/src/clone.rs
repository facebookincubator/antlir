/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::FileTimes;
use std::os::unix::fs::fchown;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use features::clone::Clone;
use tracing::trace;
use walkdir::WalkDir;
use xattr::FileExt;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

#[tracing::instrument(ret, err)]
fn copy_with_metadata(src: &Path, dst: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)?;
    if metadata.is_symlink() {
        let target = std::fs::read_link(src)?;
        std::os::unix::fs::symlink(target, dst)?;
        std::os::unix::fs::lchown(dst, Some(metadata.uid()), Some(metadata.gid()))?;
        return Ok(());
    } else if metadata.is_file() {
        std::fs::copy(src, dst)?;
    } else if metadata.is_dir() {
        std::fs::create_dir(dst)?;
    } else {
        return Err(anyhow::anyhow!(
            "not sure what to do with a non directory/file/symlink: {}",
            src.display()
        )
        .into());
    }
    trace!("read metadata: {metadata:?}");
    std::fs::set_permissions(dst, metadata.permissions())?;
    let f = std::fs::File::open(dst)?;
    // TODO(T145931158): this is actually dangerous if the file is not owned by
    // root:root, because there is no guarantee that uid X maps to the same
    // username Y in two different images (even if X and Y both exist in both
    // images!). However, antlir1 appears to completely ignore this, and we can
    // reasonably assume that existing use cases for non-root-owned cloned files
    // are for well-known uids that will match in each image, so let's punt for
    // now and solve it later.
    fchown(&f, Some(metadata.uid()), Some(metadata.gid()))?;
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    trace!("setting time to {times:?}");
    f.set_times(times)?;
    let xattrs: HashMap<_, _> = xattr::list(src)?
        .into_iter()
        .map(|key| xattr::get(src, &key).map(|val| (key, val)))
        .collect::<std::io::Result<_>>()?;
    trace!("xattrs = {xattrs:?}");
    for (key, val) in xattrs {
        if let Some(val) = val {
            f.set_xattr(key, &val)?;
        }
    }
    Ok(())
}

impl<'a> CompileFeature for Clone<'a> {
    #[tracing::instrument(name = "clone", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        // antlir2_depgraph has already done all the safety validation, so we
        // can just go ahead and blindly copy everything here
        let src_root = self
            .src_layer_info
            .as_ref()
            .expect("this is always set on antlir2+buck2")
            .subvol_symlink
            .join(self.src_path.strip_prefix("/").unwrap_or(&self.src_path))
            .canonicalize()?;
        for entry in WalkDir::new(&src_root) {
            let entry = entry.map_err(std::io::Error::from)?;
            if self.omit_outer_dir && entry.path() == src_root {
                tracing::debug!("skipping top-level dir");
                continue;
            }
            let relpath = entry
                .path()
                .strip_prefix(&src_root)
                .expect("this must be under src_root");

            // If we are cloning a directory without a trailing / into a
            // directory with a trailing /, we need to prepend the name of the
            // directory to the relpath of each entry in that src directory, so
            // that a clone like:
            //   clone(src=path/to/src, dst=/into/dir/)
            // produces files like /into/dir/src/foo
            // instead of /into/dir/foo
            let relpath: Cow<'_, Path> = if self.pre_existing_dest && !self.omit_outer_dir {
                Cow::Owned(
                    Path::new(self.src_path.file_name().expect("must have file_name"))
                        .join(relpath),
                )
            } else {
                Cow::Borrowed(relpath)
            };

            copy_with_metadata(
                entry.path(),
                &ctx.dst_path(self.dst_path.path().join(relpath.as_ref())),
            )?;
        }
        Ok(())
    }
}
