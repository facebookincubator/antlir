/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use antlir2_features::clone::Clone;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use anyhow::Context;
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
            .src_layer
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

            let dst_path = ctx.dst_path(self.dst_path.path().join(relpath.as_ref()));
            copy_with_metadata(entry.path(), &dst_path)?;

            // {ug}ids might not map to the same names in both images, so make
            // sure that we look up the src ids and copy the _names_ instead of
            // just the ids

            let src_userdb: EtcPasswd =
                std::fs::read_to_string(self.src_layer.subvol_symlink.join("etc/passwd"))
                    .and_then(|s| s.parse().map_err(std::io::Error::other))
                    .unwrap_or_else(|_| Default::default());
            let src_groupdb: EtcGroup =
                std::fs::read_to_string(self.src_layer.subvol_symlink.join("etc/group"))
                    .and_then(|s| s.parse().map_err(std::io::Error::other))
                    .unwrap_or_else(|_| Default::default());

            let meta = entry.metadata().map_err(std::io::Error::from)?;

            let new_uid = ctx.uid(
                &src_userdb
                    .get_user_by_id(meta.uid().into())
                    .with_context(|| format!("src_layer missing passwd entry for {}", meta.uid()))?
                    .name,
            )?;
            let new_gid = ctx.gid(
                &src_groupdb
                    .get_group_by_id(meta.gid().into())
                    .with_context(|| format!("src_layer missing group entry for {}", meta.gid()))?
                    .name,
            )?;

            tracing::trace!("lchown {}:{} {}", new_uid, new_gid, dst_path.display());
            std::os::unix::fs::lchown(&dst_path, Some(new_uid.into()), Some(new_gid.into()))?;
        }
        Ok(())
    }
}
