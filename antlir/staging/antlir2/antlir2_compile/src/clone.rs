/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::path::Path;

use features::clone::Clone;
use walkdir::WalkDir;

use crate::util::copy_with_metadata;
use crate::CompileFeature;
use crate::CompilerContext;
use crate::Result;

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
