/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::chown;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::FsEntry;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path as PathItem;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::stat::Mode;
use antlir2_features::types::GroupName;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Mknod;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Mknod {
    pub dst: PathInLayer,
    pub major: u64,
    pub minor: u64,
    pub user: UserName,
    pub group: GroupName,
    pub mode: Mode,
    #[serde(rename = "type")]
    pub ty: DeviceType,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum DeviceType {
    #[serde(rename = "block")]
    BlockDevice,
    #[serde(rename = "char")]
    CharDevice,
}

impl Mknod {
    fn file_type(&self) -> FileType {
        match self.ty {
            DeviceType::BlockDevice => FileType::BlockDevice,
            DeviceType::CharDevice => FileType::CharDevice,
        }
    }
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Mknod {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(PathItem::Entry(FsEntry {
            path: self.dst.to_owned().into(),
            file_type: self.file_type(),
            mode: self.mode.as_raw(),
        }))])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut v = vec![
            Requirement::ordered(ItemKey::User(self.user.clone().into()), Validator::Exists),
            Requirement::ordered(ItemKey::Group(self.group.clone().into()), Validator::Exists),
        ];
        if let Some(parent) = self.dst.parent() {
            v.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned().into()),
                Validator::FileType(FileType::Directory),
            ));
        }
        Ok(v)
    }
}

impl antlir2_compile::CompileFeature for Mknod {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let dst = ctx.dst_path(&self.dst)?;
        let kind = match self.ty {
            DeviceType::BlockDevice => nix::sys::stat::SFlag::S_IFBLK,
            DeviceType::CharDevice => nix::sys::stat::SFlag::S_IFCHR,
        };
        let perm = nix::sys::stat::Mode::from_bits_truncate(self.mode.as_raw());
        let uid = ctx.uid(&self.user)?;
        let gid = ctx.gid(&self.group)?;
        let dev = nix::sys::stat::makedev(self.major, self.minor);

        tracing::debug!("creating special file");
        nix::sys::stat::mknod(&dst, kind, perm, dev).map_err(std::io::Error::from)?;

        tracing::debug!("changing ownership of the special file");
        chown(&dst, Some(uid.into()), Some(gid.into())).map_err(std::io::Error::from)?;

        Ok(())
    }
}
