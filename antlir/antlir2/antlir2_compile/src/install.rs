/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::FileTimes;
use std::fs::Permissions;
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
            if !self.is_dir() {
                return Err(Error::InstallSrcIsDirectoryButNotDst {
                    src: self.src.to_path_buf(),
                    dst: self.dst.to_path_buf(),
                });
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

                // the depgraph already ensured that there are no conflicts, so if
                // this exists then it must have the correct contents
                if dst_path.exists() {
                    tracing::debug!(
                        dst_path = dst_path.display().to_string(),
                        "install destination already exists"
                    );
                    continue;
                }

                // For installing directories, we chown all the copied files as root.
                // Otherwise the files would be owned by the build user, which is
                // most certainly not what was intended.  Until we have a better
                // way of describing in the API what user should own the copied
                // directory and all of the contents, we replicate the current
                // Antlir1 behavior, which is to have everything owned by root.
                copy_with_metadata(entry.path(), &dst_path, Some(0), Some(0))?;
            }
        } else {
            if self.is_dir() {
                return Err(Error::InstallDstIsDirectoryButNotSrc {
                    src: self.src.to_path_buf(),
                    dst: self.dst.to_path_buf(),
                });
            }
            let dst = ctx.dst_path(&self.dst);
            if self.dev_mode {
                // If we are installing a buck-built binary in @mode/dev, it must be
                // executed from the exact same path so that it can find relatively
                // located .so libraries. There are two ways to do this:
                // 1) make a symlink to the binary
                // 2) install a shell script that `exec`s the real binary at the right
                // path
                //
                // Antlir2 chooses option 1, since it's substantially simpler and does
                // not require any assumptions about the layer (like /bin/sh even
                // existing).
                let src_abspath = std::fs::canonicalize(&self.src)?;
                std::os::unix::fs::symlink(src_abspath, &dst)?;
            } else {
                std::fs::copy(&self.src, &dst)?;
                let uid = ctx.uid(self.user.name())?;
                let gid = ctx.gid(self.group.name())?;

                let dst_file = File::options().write(true).open(&dst)?;
                fchown(&dst_file, Some(uid.into()), Some(gid.into()))
                    .map_err(std::io::Error::from)?;
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
        }
        Ok(())
    }
}
