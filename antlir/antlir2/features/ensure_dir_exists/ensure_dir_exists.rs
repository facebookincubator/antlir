/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::Permissions;
use std::os::unix::fs::chown;
use std::os::unix::fs::PermissionsExt;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::FsEntry;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::stat::Mode;
use antlir2_features::types::GroupName;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = EnsureDirExists;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct EnsureDirExists {
    pub group: GroupName,
    pub dir: PathInLayer,
    pub mode: Mode,
    pub user: UserName,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for EnsureDirExists {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(Path::Entry(FsEntry {
            path: self.dir.clone(),
            file_type: FileType::Directory,
            mode: self.mode.0,
        }))])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut v = vec![
            Requirement::ordered(ItemKey::User(self.user.clone()), Validator::Exists),
            Requirement::ordered(ItemKey::Group(self.group.clone()), Validator::Exists),
        ];
        if let Some(parent) = self.dir.parent() {
            v.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned()),
                Validator::FileType(FileType::Directory),
            ));
        }
        Ok(v)
    }
}

impl antlir2_compile::CompileFeature for EnsureDirExists {
    #[tracing::instrument(name = "ensure_dir_exists", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let dst = ctx.dst_path(&self.dir)?;
        tracing::trace!("creating {}", dst.display());
        match std::fs::create_dir(&dst) {
            Ok(_) => {
                let uid = ctx.uid(&self.user)?;
                let gid = ctx.gid(&self.group)?;
                chown(&dst, Some(uid.into()), Some(gid.into())).map_err(std::io::Error::from)?;
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
        Ok(())
    }
}
