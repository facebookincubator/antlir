/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::os::unix::prelude::MetadataExt;

use antlir2_features::Data;
use antlir2_features::Feature;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use walkdir::WalkDir;

use super::ItemKey;
use super::Requirement;
use super::Validator;
use crate::item::FileType;
use crate::item::FsEntry;
use crate::item::Group;
use crate::item::Item;
use crate::item::Path;
use crate::item::User;
use crate::Graph;

pub(crate) trait FeatureExt<'f> {
    /// List of what [Item]s this [Feature] provides. Added to the graph before
    /// any [Requirement]s so that edges work.
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(Default::default())
    }

    /// List of what [Item]s this [Feature] requires to be provided by other
    /// features / parent images.
    fn requires(&self) -> Vec<Requirement<'f>> {
        Default::default()
    }
}

impl<'f> FeatureExt<'f> for Feature<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        match &self.data {
            Data::Clone(x) => x.provides(),
            Data::EnsureDirSymlink(x) => x.provides(),
            Data::EnsureDirExists(x) => x.provides(),
            Data::EnsureFileSymlink(x) => x.provides(),
            Data::Extract(x) => x.provides(),
            Data::Genrule(_) => Ok(vec![]),
            Data::Group(x) => x.provides(),
            Data::Install(x) => x.provides(),
            Data::Mount(x) => x.provides(),
            Data::Remove(x) => x.provides(),
            Data::Requires(x) => x.provides(),
            Data::Rpm(x) => x.provides(),
            Data::Tarball(_) => todo!(),
            Data::User(x) => x.provides(),
            Data::UserMod(x) => x.provides(),
            #[cfg(facebook)]
            Data::ChefSolo(_) => Ok(Vec::new()),
        }
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        match &self.data {
            Data::Clone(x) => x.requires(),
            Data::EnsureDirSymlink(x) => x.requires(),
            Data::EnsureDirExists(x) => x.requires(),
            Data::EnsureFileSymlink(x) => x.requires(),
            Data::Extract(x) => x.requires(),
            Data::Genrule(_) => vec![],
            Data::Group(x) => x.requires(),
            Data::Install(x) => x.requires(),
            Data::Mount(x) => x.requires(),
            Data::Remove(x) => x.requires(),
            Data::Requires(x) => x.requires(),
            Data::Rpm(x) => x.requires(),
            Data::Tarball(_) => todo!(),
            Data::User(x) => x.requires(),
            Data::UserMod(x) => x.requires(),
            #[cfg(facebook)]
            Data::ChefSolo(_) => Vec::new(),
        }
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::clone::Clone<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement::ordered(
            ItemKey::Layer(self.src_layer.label.to_owned()),
            Validator::ItemInLayer {
                key: ItemKey::Path(self.src_path.path().to_owned().into()),
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
                ItemKey::Path(self.dst_path.path().to_owned().into()),
                Validator::FileType(FileType::Directory),
            ));
        } else {
            v.push(Requirement::ordered(
                ItemKey::Path(
                    self.dst_path
                        .path()
                        .parent()
                        .expect("Clone with pre_existing_dst will always have parent")
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            ));
        }
        // Files we clone will usually be owned by root:root, but not always! To
        // be safe we have to make sure that all discovered users and groups
        // exist in this destination layer
        let mut uids = HashSet::new();
        let mut gids = HashSet::new();
        for entry in WalkDir::new(
            self.src_layer.subvol_symlink.join(
                self.src_path
                    .strip_prefix("/")
                    .unwrap_or(self.src_path.path()),
            ),
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
                        .clone(),
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
                        .clone(),
                ),
                Validator::Exists,
            ));
        }
        v
    }

    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        let src_layer_depgraph_path = &self.src_layer.depgraph.as_ref();
        let src_layer = std::fs::read(src_layer_depgraph_path).map_err(|e| {
            format!(
                "could not read src_layer depgraph '{}': {e}",
                src_layer_depgraph_path.display()
            )
        })?;
        let src_depgraph: Graph<'_> = serde_json::from_slice(&src_layer)
            .map_err(|e| format!("could not deserialize src_layer depgraph: {e}"))?;
        let mut v = Vec::new();
        // if this is creating the top-level dest, we need to produce that now
        if !self.pre_existing_dest {
            match src_depgraph.get_item(&ItemKey::Path(self.src_path.path().to_owned().into())) {
                Some(Item::Path(Path::Entry(entry))) => {
                    v.push(Item::Path(Path::Entry(FsEntry {
                        path: self.dst_path.path().to_owned().into(),
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
        for key in src_depgraph.items.keys() {
            if let ItemKey::Path(p) = key {
                if self.omit_outer_dir && p == self.src_path.path() {
                    continue;
                }

                if let Ok(relpath) = p.strip_prefix(self.src_path.path()) {
                    // If we are cloning a directory without a trailing / into a
                    // directory with a trailing /, we need to prepend the name of the
                    // directory to the relpath of each entry in that src directory, so
                    // that a clone like:
                    //   clone(src=path/to/src, dst=/into/dir/)
                    // produces files like /into/dir/src/foo
                    // instead of /into/dir/foo
                    let relpath: Cow<'_, std::path::Path> =
                        if self.pre_existing_dest && !self.omit_outer_dir {
                            Cow::Owned(
                                std::path::Path::new(
                                    self.src_path.file_name().expect("must have file_name"),
                                )
                                .join(relpath),
                            )
                        } else {
                            Cow::Borrowed(relpath)
                        };
                    let dst_path = self.dst_path.join(&relpath);
                    if let Some(Item::Path(Path::Entry(entry))) = src_depgraph.get_item(key) {
                        v.push(Item::Path(Path::Entry(FsEntry {
                            path: dst_path.into(),
                            file_type: entry.file_type,
                            mode: entry.mode,
                        })));
                    }
                }
            }
        }

        Ok(v)
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::ensure_dir_exists::EnsureDirExists<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(vec![Item::Path(Path::Entry(FsEntry {
            path: self.dir.path().to_owned().into(),
            file_type: FileType::Directory,
            mode: self.mode.0,
        }))])
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![
            Requirement::ordered(
                ItemKey::User(self.user.name().to_owned().into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Group(self.group.name().to_owned().into()),
                Validator::Exists,
            ),
        ];
        if let Some(parent) = self.dir.parent() {
            v.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned().into()),
                Validator::FileType(FileType::Directory),
            ));
        }
        v
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::extract::Extract<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        match self {
            Self::Layer(l) => l
                .binaries
                .iter()
                .flat_map(|path| {
                    vec![
                        Requirement::ordered(
                            ItemKey::Layer(l.layer.label.to_owned()),
                            Validator::ItemInLayer {
                                key: ItemKey::Path(path.path().to_owned().into()),
                                validator: Box::new(Validator::Executable),
                            },
                        ),
                        Requirement::ordered(
                            ItemKey::Path(
                                path.path()
                                    .parent()
                                    .expect("dst always has parent")
                                    .to_owned()
                                    .into(),
                            ),
                            Validator::FileType(FileType::Directory),
                        ),
                    ]
                })
                .collect(),
            Self::Buck(b) => vec![Requirement::ordered(
                ItemKey::Path(
                    b.dst
                        .path()
                        .parent()
                        .expect("dst always has parent")
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            )],
        }
    }

    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(match self {
            Self::Layer(l) => l
                .binaries
                .iter()
                .map(|path| {
                    Item::Path(Path::Entry(FsEntry {
                        path: path.path().to_owned().into(),
                        file_type: FileType::File,
                        mode: 0o555,
                    }))
                })
                .collect(),
            Self::Buck(b) => {
                vec![Item::Path(Path::Entry(FsEntry {
                    path: b.dst.path().to_owned().into(),
                    file_type: FileType::File,
                    mode: 0o555,
                }))]
            }
        })
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::install::Install<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        if self.is_dir() {
            let mut v = vec![Item::Path(Path::Entry(FsEntry {
                path: self.dst.path().to_owned().into(),
                file_type: FileType::Directory,
                mode: self.mode.as_raw(),
            }))];
            for entry in WalkDir::new(&self.src) {
                let entry = entry
                    .map_err(|e| format!("could not walk src dir {}: {e}", self.src.display()))?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this has to be under src");
                if relpath == std::path::Path::new("") {
                    continue;
                }
                if entry.file_type().is_file() {
                    v.push(Item::Path(Path::Entry(FsEntry {
                        path: self.dst.join(relpath).into(),
                        file_type: FileType::File,
                        mode: 0o444,
                    })))
                } else if entry.file_type().is_dir() {
                    v.push(Item::Path(Path::Entry(FsEntry {
                        path: self.dst.join(relpath).into(),
                        file_type: FileType::Directory,
                        mode: 0o555,
                    })))
                } else if entry.file_type().is_symlink() {
                    let target = std::fs::read_link(entry.path()).map_err(|_e| {
                        format!("could not get link target of {}", entry.path().display())
                    })?;
                    v.push(Item::Path(Path::Symlink {
                        link: self.dst.join(relpath).into(),
                        target: target.into(),
                    }));
                }
            }
            Ok(v)
        } else {
            Ok(vec![Item::Path(Path::Entry(FsEntry {
                path: self.dst.path().to_owned().into(),
                file_type: FileType::File,
                mode: self.mode.as_raw(),
            }))])
        }
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![
            Requirement::ordered(
                ItemKey::User(self.user.name().to_owned().into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Group(self.group.name().to_owned().into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Path(
                    self.dst
                        .path()
                        .parent()
                        .expect("Install dst will always have parent")
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            ),
        ]
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::mount::Mount<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement::ordered(
            ItemKey::Path(self.mountpoint().path().to_owned().into()),
            Validator::FileType(match self.is_directory() {
                true => FileType::Directory,
                false => FileType::File,
            }),
        )];
        match self {
            Self::Layer(l) => v.push(Requirement::ordered(
                ItemKey::Layer(l.src.label.to_owned()),
                Validator::Exists,
            )),
            Self::Host(_) => (),
        }
        v
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::remove::Remove<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(vec![Item::Path(Path::Removed(
            self.path.path().to_owned().into(),
        ))])
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        match self.must_exist {
            false => vec![],
            true => vec![Requirement::ordered(
                ItemKey::Path(self.path.path().to_owned().into()),
                Validator::Exists,
            )],
        }
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::requires::Requires<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        self.files
            .iter()
            .map(|p| {
                Requirement::ordered(
                    ItemKey::Path(p.path().to_owned().into()),
                    Validator::FileType(FileType::File),
                )
            })
            .chain(self.users.iter().map(|u| {
                Requirement::ordered(ItemKey::User(u.name().to_owned().into()), Validator::Exists)
            }))
            .chain(self.groups.iter().map(|g| {
                Requirement::ordered(
                    ItemKey::Group(g.name().to_owned().into()),
                    Validator::Exists,
                )
            }))
            .collect()
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::rpms::Rpm<'f> {}

impl<'f> FeatureExt<'f> for antlir2_features::symlink::Symlink<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(vec![Item::Path(Path::Symlink {
            link: self.link.path().to_owned().into(),
            target: self.target.path().to_owned().into(),
        })])
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        // target may be a relative path, in which
        // case we need to resolve it relative to
        // the link
        let absolute_target = match self.target.path().is_absolute() {
            true => self.target.path().to_owned(),
            false => self
                .link
                .path()
                .parent()
                .expect("the link cannot itself be /")
                .join(self.target.path()),
        };
        vec![
            Requirement::ordered(
                ItemKey::Path(absolute_target.into()),
                Validator::FileType(match self.is_directory {
                    true => FileType::Directory,
                    false => FileType::File,
                }),
            ),
            Requirement::ordered(
                ItemKey::Path(
                    self.link
                        .path()
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("/"))
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            ),
        ]
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::usergroup::Group<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(vec![Item::Group(Group {
            name: self.name.name().to_owned().into(),
        })])
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![Requirement::ordered(
            ItemKey::Path(std::path::Path::new("/etc/group").into()),
            Validator::Exists,
        )]
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::usergroup::User<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>, String> {
        Ok(vec![Item::User(User {
            name: self.name.name().to_owned().into(),
        })])
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![
            Requirement::unordered(
                ItemKey::Path(self.home_dir.path().to_owned().into()),
                Validator::FileType(FileType::Directory),
            ),
            Requirement::unordered(
                ItemKey::Path(self.shell.path().to_owned().into()),
                Validator::Executable,
            ),
            Requirement::ordered(
                ItemKey::Path(std::path::Path::new("/etc/passwd").into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Path(std::path::Path::new("/etc/group").into()),
                Validator::Exists,
            ),
        ];
        v.extend(
            self.supplementary_groups
                .iter()
                .chain(vec![&self.primary_group])
                .map(|g| {
                    Requirement::ordered(
                        ItemKey::Group(g.name().to_owned().into()),
                        Validator::Exists,
                    )
                }),
        );
        v
    }
}

impl<'f> FeatureExt<'f> for antlir2_features::usergroup::UserMod<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement::ordered(
            ItemKey::User(self.username.name().to_owned().into()),
            Validator::Exists,
        )];
        v.extend(self.add_supplementary_groups.iter().map(|g| {
            Requirement::ordered(
                ItemKey::Group(g.name().to_owned().into()),
                Validator::Exists,
            )
        }));
        v
    }
}
