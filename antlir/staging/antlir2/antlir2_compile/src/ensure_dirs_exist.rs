/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::Permissions;
use std::os::unix::fs::chown;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use features::ensure_dirs_exist::EnsureDirsExist;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

impl<'a> CompileFeature for EnsureDirsExist<'a> {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let ancestors: Vec<_> = self
            .subdirs_to_create
            .ancestors()
            .filter(|p| *p != Path::new("/"))
            .collect();
        for relpath in ancestors.into_iter().rev() {
            let dst = ctx.dst_path(self.into_dir.join(relpath));
            tracing::trace!("creating {} for {}", dst.display(), relpath.display());
            match std::fs::create_dir(&dst) {
                Ok(_) => {
                    let uid = ctx.uid(self.user.name())?;
                    let gid = ctx.gid(self.group.name())?;
                    chown(&dst, Some(uid.into()), Some(gid.into()))
                        .map_err(std::io::Error::from)?;
                    std::fs::set_permissions(&dst, Permissions::from_mode(self.mode.0))?;
                }
                Err(e) => match e.kind() {
                    // The directory may have already been created by a concurrent [EnsureDirsExist]
                    // This is safe to ignore because the depgraph will already
                    // have validated that the ownership and modes are identical
                    std::io::ErrorKind::AlreadyExists => {
                        tracing::debug!(dst = dst.display().to_string(), "dir already existed");
                    }
                    _ => return Err(e.into()),
                },
            }
        }
        Ok(())
    }
}
