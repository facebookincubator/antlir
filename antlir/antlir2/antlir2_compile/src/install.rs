/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::FileTimes;
use std::fs::Permissions;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::fchown;
use std::os::unix::fs::PermissionsExt;

use antlir2_features::install::Install;
use tracing::debug;
use walkdir::WalkDir;

use crate::util::copy_with_metadata;
use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

impl<'a> CompileFeature for Install<'a> {
    #[tracing::instrument(name = "install", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        if self.src.is_dir() {
            debug!("{:?} is a dir", self.src);
            if self.dst.as_os_str().as_bytes().last().copied() != Some(b'/') {
                return Err(Error::InstallSrcIsDirectoryButNotDst(
                    self.src.to_path_buf(),
                    self.dst.to_path_buf(),
                ));
            }
            for entry in WalkDir::new(&self.src) {
                let entry = entry.map_err(std::io::Error::from)?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this must be under src");

                debug!("relpath is {relpath:?}");

                let dst_path = ctx.dst_path(self.dst.path().join(relpath));
                debug!("dst path is {dst_path:?}");
                // For installing directories, we chown all the copied files as root.
                // Otherwise the files would be owned by the build user, which is
                // most certainly not what was intended.  Until we have a better
                // way of describing in the API what user should own the copied
                // directory and all of the contents, we replicate the current
                // Antlir1 behavior, which is to have everything owned by root.
                copy_with_metadata(entry.path(), &dst_path, Some(0), Some(0))?;
            }
        } else {
            let dst = ctx.dst_path(&self.dst);
            std::fs::copy(&self.src, &dst)?;
            let uid = ctx.uid(self.user.name())?;
            let gid = ctx.gid(self.group.name())?;

            let dst_file = File::options().write(true).open(&dst)?;
            fchown(&dst_file, Some(uid.into()), Some(gid.into())).map_err(std::io::Error::from)?;
            dst_file.set_permissions(Permissions::from_mode(self.mode.as_raw()))?;

            // Sync the file times with the source. This is not strictly necessary
            // but does lead to some better reproducibility of image builds as it's
            // one less entropic thing to change between runs when the input did not
            // change
            let src_meta = std::fs::metadata(&self.src)?;
            let times = FileTimes::new()
                .set_accessed(src_meta.accessed()?)
                .set_modified(src_meta.modified()?);
            dst_file.set_times(times)?;
        }
        Ok(())
    }
}
