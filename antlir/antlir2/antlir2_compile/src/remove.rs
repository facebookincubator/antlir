/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_features::remove::Remove;
use tracing::trace;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

impl<'a> CompileFeature for Remove<'a> {
    #[tracing::instrument(name = "remove", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let path = ctx.dst_path(&self.path);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) => {
                if !self.must_exist && e.kind() == std::io::ErrorKind::NotFound {
                    trace!("'{}' did not exist", self.path.display());
                    Ok(())
                } else if e.kind() == std::io::ErrorKind::IsADirectory {
                    std::fs::remove_dir_all(&path).map_err(Error::from)
                } else {
                    Err(e.into())
                }
            }
        }
    }
}
