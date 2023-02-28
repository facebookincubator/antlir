/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_features::symlink::Symlink;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

impl<'a> CompileFeature for Symlink<'a> {
    #[tracing::instrument(name = "symlink", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        // Unlike antlir1, we don't have to do all the paranoid checking,
        // because the new depgraph will have done it all for us already.
        // I am also choosing to do preserve absolute symlinks if that's what
        // the user asked for, since it's more intuitive when the image is
        // installed somewhere and used as a rootfs, and doing things "inside"
        // the image without actually doing some form of chroot is super broken
        // anyway.
        std::os::unix::fs::symlink(self.target.path(), ctx.dst_path(self.link.path()))?;
        Ok(())
    }
}
