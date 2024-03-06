/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use antlir2_compile::util::copy_with_metadata;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::FsEntry;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path as PathItem;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_depgraph::Graph;
use antlir2_features::types::GroupName;
use antlir2_features::types::LayerInfo;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use walkdir::WalkDir;

pub type Feature = Clone;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Clone {
    pub src_layer: LayerInfo,
    pub omit_outer_dir: bool,
    pub pre_existing_dest: bool,
    pub src_path: PathInLayer,
    pub dst_path: PathInLayer,
    #[serde(default)]
    pub usergroup: Option<CloneUserGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct CloneUserGroup {
    pub user: UserName,
    pub group: GroupName,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Clone {
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut v = vec![Requirement::ordered(
            ItemKey::Layer(self.src_layer.label.to_owned()),
            Validator::ItemInLayer {
                key: ItemKey::Path(self.src_path.to_owned().into()),
                validator: Box::new(if self.omit_outer_dir {
                    Validator::FileType(FileType::Directory)
                } else {
                    // If 'omit_outer_dir' is false, it doesn't matter if
                    // src_path is a file or directory, just that it exists
                    Validator::Exists
                }),
            },
        )];
        if self.pre_existing_dest {
            v.push(Requirement::ordered(
                ItemKey::Path(self.dst_path.to_owned().into()),
                Validator::FileType(FileType::Directory),
            ));
        } else {
            v.push(Requirement::ordered(
                ItemKey::Path(
                    self.dst_path
                        .parent()
                        .expect("Clone with pre_existing_dst will always have parent")
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            ));
        }
        if let Some(usergroup) = &self.usergroup {
            v.push(Requirement::ordered(
                ItemKey::User(usergroup.user.clone().into()),
                Validator::Exists,
            ));
            v.push(Requirement::ordered(
                ItemKey::Group(usergroup.group.clone().into()),
                Validator::Exists,
            ));
        } else {
            // Files we clone will usually be owned by root:root, but not always! To
            // be safe we have to make sure that all discovered users and groups
            // exist in this destination layer
            let mut uids = HashSet::new();
            let mut gids = HashSet::new();
            for entry in WalkDir::new(
                self.src_layer
                    .subvol_symlink
                    .join(self.src_path.strip_prefix("/").unwrap_or(&self.src_path)),
            ) {
                // ignore any errors, they'll surface again later at a more
                // appropriate place than this user/group id collection process
                if let Ok(metadata) = entry.and_then(|e| e.metadata()) {
                    uids.insert(metadata.uid());
                    gids.insert(metadata.gid());
                }
            }
            let users: EtcPasswd =
                std::fs::read_to_string(self.src_layer.subvol_symlink.join("etc/passwd"))
                    .and_then(|s| s.parse().map_err(std::io::Error::other))
                    .unwrap_or_else(|_| Default::default());
            let groups: EtcGroup =
                std::fs::read_to_string(self.src_layer.subvol_symlink.join("etc/group"))
                    .and_then(|s| s.parse().map_err(std::io::Error::other))
                    .unwrap_or_else(|_| Default::default());
            for uid in uids {
                v.push(Requirement::ordered(
                    ItemKey::User(
                        users
                            .get_user_by_id(uid.into())
                            .expect("this layer could not have been built if this uid is missing")
                            .name
                            .clone()
                            .into_owned(),
                    ),
                    Validator::Exists,
                ));
            }
            for gid in gids {
                v.push(Requirement::ordered(
                    ItemKey::Group(
                        groups
                            .get_group_by_id(gid.into())
                            .expect("this layer could not have been built if this gid is missing")
                            .name
                            .clone()
                            .into_owned(),
                    ),
                    Validator::Exists,
                ));
            }
        }
        Ok(v)
    }

    fn provides(&self) -> Result<Vec<Item>, String> {
        let src_layer_depgraph_path: &Path = self.src_layer.depgraph.as_ref();
        let src_layer = std::fs::read(src_layer_depgraph_path)
            .context("while reading src_layer depgraph")
            .map_err(|e| e.to_string())?;
        let src_depgraph: Graph = serde_json::from_slice(&src_layer)
            .context("while parsing src_layer depgraph")
            .map_err(|e| e.to_string())?;
        let mut v = Vec::new();
        // if this is creating the top-level dest, we need to produce that now
        if !self.pre_existing_dest {
            match src_depgraph.get_item(&ItemKey::Path(self.src_path.to_owned().into())) {
                Some(Item::Path(PathItem::Entry(entry))) => {
                    v.push(Item::Path(PathItem::Entry(FsEntry {
                        path: self.dst_path.to_owned().into(),
                        file_type: entry.file_type,
                        mode: entry.mode,
                    })));
                }
                // If we couldn't find it in the src_layer (or if it wasn't a
                // path entry), don't do anything. The error message produced by
                // the unsatisfied validator will be much clearer to the user
                _ => {}
            }
        }
        // find any files or directories that appear underneath the clone source
        for key in src_depgraph.items_keys() {
            if let ItemKey::Path(p) = key {
                if self.omit_outer_dir && p == self.src_path.as_path() {
                    continue;
                }

                if let Ok(relpath) = p.strip_prefix(&self.src_path) {
                    // If we are cloning a directory without a trailing / into a
                    // directory with a trailing /, we need to prepend the name of the
                    // directory to the relpath of each entry in that src directory, so
                    // that a clone like:
                    //   clone(src=path/to/src, dst=/into/dir/)
                    // produces files like /into/dir/src/foo
                    // instead of /into/dir/foo
                    let relpath = if self.pre_existing_dest && !self.omit_outer_dir {
                        Path::new(self.src_path.file_name().expect("must have file_name"))
                            .join(relpath)
                    } else {
                        relpath.to_owned()
                    };
                    let dst_path = self.dst_path.join(&relpath);
                    if let Some(Item::Path(path_item)) = src_depgraph.get_item(key) {
                        v.push(Item::Path(match path_item {
                            PathItem::Entry(entry) => PathItem::Entry(FsEntry {
                                path: dst_path.into(),
                                file_type: entry.file_type,
                                mode: entry.mode,
                            }),
                            PathItem::Symlink { link: _, target } => PathItem::Symlink {
                                link: dst_path,
                                target: target.to_owned(),
                            },
                            PathItem::Mount(_) => {
                                return Err(format!(
                                    "mount paths cannot be cloned: {}",
                                    dst_path.display()
                                ));
                            }
                        }));
                    }
                }
            }
        }

        Ok(v)
    }
}

impl antlir2_compile::CompileFeature for Clone {
    #[tracing::instrument(name = "clone", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        // antlir2_depgraph has already done all the safety validation, so we
        // can just go ahead and blindly copy everything here
        let src_root = self
            .src_layer
            .subvol_symlink
            .join(self.src_path.strip_prefix("/").unwrap_or(&self.src_path))
            .canonicalize()?;
        for entry in WalkDir::new(&src_root) {
            let entry = entry.map_err(std::io::Error::from)?;
            if self.omit_outer_dir && entry.path() == src_root.as_path() {
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

            let dst_path = ctx.dst_path(self.dst_path.join(relpath.as_ref()))?;
            copy_with_metadata(entry.path(), &dst_path, None, None)?;

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

            let (new_uid, new_gid) = match &self.usergroup {
                Some(usergroup) => (
                    ctx.uid(
                        &src_userdb
                            .get_user_by_name(&usergroup.user)
                            .with_context(|| {
                                format!("src_layer missing passwd entry for {}", usergroup.user)
                            })?
                            .name,
                    )?,
                    ctx.gid(
                        &src_groupdb
                            .get_group_by_name(&usergroup.group)
                            .with_context(|| {
                                format!("src_layer missing group entry for {}", usergroup.group)
                            })?
                            .name,
                    )?,
                ),
                None => (
                    ctx.uid(
                        &src_userdb
                            .get_user_by_id(meta.uid().into())
                            .with_context(|| {
                                format!("src_layer missing passwd entry for {}", meta.uid())
                            })?
                            .name,
                    )?,
                    ctx.gid(
                        &src_groupdb
                            .get_group_by_id(meta.gid().into())
                            .with_context(|| {
                                format!("src_layer missing group entry for {}", meta.gid())
                            })?
                            .name,
                    )?,
                ),
            };

            tracing::trace!("lchown {}:{} {}", new_uid, new_gid, dst_path.display());
            std::os::unix::fs::lchown(&dst_path, Some(new_uid.into()), Some(new_gid.into()))?;
        }
        Ok(())
    }
}
