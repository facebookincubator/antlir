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

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

impl<'a> CompileFeature for Install<'a> {
    #[tracing::instrument(name = "install", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
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
        let src = std::fs::metadata(&self.src)?;
        let times = FileTimes::new()
            .set_accessed(src.accessed()?)
            .set_modified(src.modified()?);
        dst_file.set_times(times)?;

        Ok(())
    }
}
