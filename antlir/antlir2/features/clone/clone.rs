/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashMap;
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
use antlir2_facts::fact::dir_entry::DirEntry;
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
    src_layer: LayerInfo,
    omit_outer_dir: bool,
    pre_existing_dest: bool,
    src_path: PathInLayer,
    dst_path: PathInLayer,
    #[serde(default)]
    usergroup: Option<CloneUserGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct CloneUserGroup {
    user: UserName,
    group: GroupName,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Clone {
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut v = Vec::new();
        if self.pre_existing_dest {
            v.push(Requirement::ordered(
                ItemKey::Path(self.dst_path.to_owned()),
                Validator::FileType(FileType::Directory),
            ));
        } else {
            v.push(Requirement::ordered(
                ItemKey::Path(
                    self.dst_path
                        .parent()
                        .expect("Clone with pre_existing_dst will always have parent")
                        .to_owned(),
                ),
                Validator::FileType(FileType::Directory),
            ));
        }
        let src_facts =
            antlir2_facts::RoDatabase::open(&self.src_layer.facts_db, Default::default())
                .context("while opening src_layer facts db")
                .map_err(|e| format!("{e:#?}"))?;

        let root_lookup = self
            .src_path
            .to_str()
            .expect("all our paths are utf8")
            .trim_end_matches('/');
        let root_lookup = if root_lookup.is_empty() {
            "/"
        } else {
            root_lookup
        };
        if let Some(root) = src_facts
            .get::<DirEntry>(DirEntry::key(Path::new(root_lookup)))
            .with_context(|| format!("while looking up src path '{}'", self.src_path.display()))
            .map_err(|e| format!("{e:#?}"))?
        {
            // If 'omit_outer_dir' is false, it doesn't matter if src_path is a
            // file or directory, just that it exists
            if self.omit_outer_dir {
                if !matches!(root, DirEntry::Directory(_)) {
                    return Err(format!(
                        "src path '{}' is a file, but omit_outer_dir is true so it should be a directory",
                        self.src_path.display()
                    ));
                }
            }
        } else {
            return Err(format!(
                "src path '{}' does not exist in {}",
                self.src_path.display(),
                self.src_layer.label,
            ));
        }

        if let Some(usergroup) = &self.usergroup {
            v.push(Requirement::ordered(
                ItemKey::User(usergroup.user.clone()),
                Validator::Exists,
            ));
            v.push(Requirement::ordered(
                ItemKey::Group(usergroup.group.clone()),
                Validator::Exists,
            ));
        } else {
            // Files we clone will usually be owned by root:root, but not always! To
            // be safe we have to make sure that all discovered users and groups
            // exist in this destination layer
            let mut all_user_names: HashMap<u32, String> = src_facts
                .iter::<antlir2_facts::fact::user::User>()
                .map(|u| (u.id(), u.name().to_owned()))
                .collect();
            let mut all_group_names: HashMap<u32, String> = src_facts
                .iter::<antlir2_facts::fact::user::Group>()
                .map(|g| (g.id(), g.name().to_owned()))
                .collect();
            let mut need_users = HashSet::new();
            let mut need_groups = HashSet::new();
            for entry in src_facts.iter::<DirEntry>() {
                if entry.path().starts_with(&self.src_path) {
                    if let Some(name) = all_user_names.remove(&entry.uid()) {
                        need_users.insert(name);
                    }
                    if let Some(name) = all_group_names.remove(&entry.gid()) {
                        need_groups.insert(name);
                    }
                }
            }
            v.extend(
                need_users
                    .into_iter()
                    .map(|u| Requirement::ordered(ItemKey::User(u), Validator::Exists)),
            );
            v.extend(
                need_groups
                    .into_iter()
                    .map(|g| Requirement::ordered(ItemKey::Group(g), Validator::Exists)),
            );
        }
        Ok(v)
    }

    fn provides(&self) -> Result<Vec<Item>, String> {
        let src_facts =
            antlir2_facts::RoDatabase::open(&self.src_layer.facts_db, Default::default())
                .context("while opening src_layer facts db")
                .map_err(|e| format!("{e:#?}"))?;
        let mut v = Vec::new();
        // if this is creating the top-level dest, we need to produce that now
        if !self.pre_existing_dest {
            if let Some(root) = src_facts
                .get::<DirEntry>(DirEntry::key(&self.src_path))
                .with_context(|| format!("while looking up src path '{}'", self.src_path.display()))
                .map_err(|e| format!("{e:#?}"))?
            {
                let file_type = FileType::from_mode(root.mode())
                    .expect("file mode bits can always be mapped to a FileType");
                v.push(Item::Path(PathItem::Entry(FsEntry {
                    path: self.dst_path.clone(),
                    file_type,
                    mode: root.mode(),
                })));
            }
            // If we couldn't find it in the src_layer (or if it wasn't a
            // path entry), don't do anything. The error message produced by
            // the unsatisfied validator will be much clearer to the user
        }
        // find any files or directories that appear underneath the clone source
        for entry in src_facts.iter::<DirEntry>() {
            if self.omit_outer_dir && entry.path() == self.src_path.as_path() {
                continue;
            }
            if let Ok(relpath) = entry.path().strip_prefix(&self.src_path) {
                // If we are cloning a directory without a trailing / into a
                // directory with a trailing /, we need to prepend the name of the
                // directory to the relpath of each entry in that src directory, so
                // that a clone like:
                //   clone(src=path/to/src, dst=/into/dir/)
                // produces files like /into/dir/src/foo
                // instead of /into/dir/foo
                let relpath = if self.pre_existing_dest && !self.omit_outer_dir {
                    Path::new(self.src_path.file_name().expect("must have file_name")).join(relpath)
                } else {
                    relpath.to_owned()
                };
                let dst_path = self.dst_path.join(&relpath);
                let file_type = FileType::from_mode(entry.mode())
                    .expect("file mode bits can always be mapped to a FileType");

                v.push(Item::Path(match entry {
                    DirEntry::Directory(_) | DirEntry::RegularFile(_) => PathItem::Entry(FsEntry {
                        path: dst_path.clone(),
                        file_type,
                        mode: entry.mode(),
                    }),
                    DirEntry::Symlink(symlink) => PathItem::Symlink {
                        link: dst_path,
                        target: symlink.raw_target().to_owned(),
                    },
                }));
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
