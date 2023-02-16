/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use features::Data;
use features::Feature;

use super::ItemKey;
use super::Requirement;
use super::Validator;
use crate::item::FileType;
use crate::item::FsEntry;
use crate::item::Group;
use crate::item::Item;
use crate::item::Path;
use crate::item::User;

pub(crate) trait FeatureExt<'f> {
    /// List of what [Item]s this [Feature] provides. Added to the graph before
    /// any [Requirement]s so that edges work.
    fn provides(&self) -> Vec<Item<'f>> {
        Default::default()
    }

    /// List of what [Item]s this [Feature] requires to be provided by other
    /// features / parent images.
    fn requires(&self) -> Vec<Requirement<'f>> {
        Default::default()
    }
}

impl<'f> FeatureExt<'f> for Feature<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        match &self.data {
            Data::Clone(x) => x.provides(),
            Data::EnsureDirSymlink(x) => x.provides(),
            Data::EnsureDirsExist(x) => x.provides(),
            Data::EnsureFileSymlink(x) => x.provides(),
            Data::Genrule(_) => vec![],
            Data::Group(x) => x.provides(),
            Data::Install(x) => x.provides(),
            Data::Mount(x) => x.provides(),
            Data::ParentLayer(_) => vec![],
            Data::ReceiveSendstream(_) => vec![],
            Data::Remove(x) => x.provides(),
            Data::Rpm(x) => x.provides(),
            Data::Requires(x) => x.provides(),
            Data::User(x) => x.provides(),
            Data::UserMod(x) => x.provides(),
            _ => todo!("{:?}", self.data),
        }
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        match &self.data {
            Data::Clone(x) => x.requires(),
            Data::EnsureDirSymlink(x) => x.requires(),
            Data::EnsureDirsExist(x) => x.requires(),
            Data::EnsureFileSymlink(x) => x.requires(),
            Data::Genrule(_) => vec![],
            Data::Group(x) => x.requires(),
            Data::Install(x) => x.requires(),
            Data::Mount(x) => x.requires(),
            Data::ParentLayer(_) => vec![],
            Data::ReceiveSendstream(_) => vec![],
            Data::Remove(x) => x.requires(),
            Data::Rpm(x) => x.requires(),
            Data::Requires(x) => x.requires(),
            Data::User(x) => x.requires(),
            Data::UserMod(x) => x.requires(),
            _ => todo!("{:?}", self.data),
        }
    }
}

impl<'f> FeatureExt<'f> for features::clone::Clone<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement {
            key: ItemKey::Layer(self.src_layer.label().to_owned()),
            validator: Validator::Exists,
        }];
        if self.pre_existing_dest {
            v.push(Requirement {
                key: ItemKey::Path(self.dst_path.path().to_owned().into()),
                validator: Validator::FileType(FileType::Directory),
            });
        } else {
            v.push(Requirement {
                key: ItemKey::Path(
                    self.dst_path
                        .path()
                        .parent()
                        .expect("Clone with pre_existing_dst will always have parent")
                        .to_owned()
                        .into(),
                ),
                validator: Validator::FileType(FileType::Directory),
            });
        }
        v
    }
}

impl<'f> FeatureExt<'f> for features::ensure_dirs_exist::EnsureDirsExist<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        self.subdirs_to_create
            .path()
            .ancestors()
            .filter(|p| *p != std::path::Path::new("/"))
            .filter(|p| *p != std::path::Path::new(""))
            .map(|p| self.into_dir.path().join(p))
            .map(|p| {
                Item::Path(Path::Entry(FsEntry {
                    path: p.into(),
                    file_type: FileType::Directory,
                    mode: self.mode.0,
                }))
            })
            .collect()
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![
            Requirement {
                key: ItemKey::Path(self.into_dir.path().to_owned().into()),
                validator: Validator::FileType(FileType::Directory),
            },
            Requirement {
                key: ItemKey::User(self.user.name().to_owned().into()),
                validator: Validator::Exists,
            },
            Requirement {
                key: ItemKey::Group(self.group.name().to_owned().into()),
                validator: Validator::Exists,
            },
        ]
    }
}

impl<'f> FeatureExt<'f> for features::install::Install<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        vec![Item::Path(Path::Entry(FsEntry {
            path: self.dst.path().to_owned().into(),
            // TODO: technically this can be a directory sometimes too, but I
            // need to make that piped through the buck graph instead of
            // something only discoverable at runtime
            file_type: FileType::File,
            mode: self
                .mode
                .expect("TODO: ensure this is always set in buck")
                .0,
        }))]
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![
            Requirement {
                key: ItemKey::User(self.user.name().to_owned().into()),
                validator: Validator::Exists,
            },
            Requirement {
                key: ItemKey::Group(self.group.name().to_owned().into()),
                validator: Validator::Exists,
            },
            Requirement {
                key: ItemKey::Path(
                    self.dst
                        .path()
                        .parent()
                        .expect("Install dst will always have parent")
                        .to_owned()
                        .into(),
                ),
                validator: Validator::FileType(FileType::Directory),
            },
        ]
    }
}

impl<'f> FeatureExt<'f> for features::mount::Mount<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement {
            key: ItemKey::Path(self.mountpoint().path().to_owned().into()),
            validator: Validator::FileType(match self.is_directory() {
                true => FileType::Directory,
                false => FileType::File,
            }),
        }];
        match self {
            Self::Layer(l) => v.push(Requirement {
                key: ItemKey::Layer(l.src.label().to_owned()),
                validator: Validator::Exists,
            }),
            Self::Host(_) => (),
        }
        v
    }
}

impl<'f> FeatureExt<'f> for features::remove::Remove<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        vec![Item::Path(Path::Removed(
            self.path.path().to_owned().into(),
        ))]
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        match self.must_exist {
            false => vec![],
            true => vec![Requirement {
                key: ItemKey::Path(self.path.path().to_owned().into()),
                validator: Validator::Exists,
            }],
        }
    }
}

impl<'f> FeatureExt<'f> for features::requires::Requires<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        self.files
            .iter()
            .map(|p| Requirement {
                key: ItemKey::Path(p.path().to_owned().into()),
                validator: Validator::FileType(FileType::File),
            })
            .chain(self.users.iter().map(|u| Requirement {
                key: ItemKey::User(u.name().to_owned().into()),
                validator: Validator::Exists,
            }))
            .chain(self.groups.iter().map(|g| Requirement {
                key: ItemKey::Group(g.name().to_owned().into()),
                validator: Validator::Exists,
            }))
            .collect()
    }
}

impl<'f> FeatureExt<'f> for features::rpms::Rpm<'f> {}

impl<'f> FeatureExt<'f> for features::symlink::Symlink<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        vec![Item::Path(Path::Entry(FsEntry {
            path: self.link.path().to_owned().into(),
            file_type: match self.is_directory {
                true => FileType::Directory,
                false => FileType::File,
            },
            // symlink mode does not matter
            mode: 0o777,
        }))]
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![
            Requirement {
                key: ItemKey::Path(self.target.path().to_owned().into()),
                validator: Validator::FileType(match self.is_directory {
                    true => FileType::Directory,
                    false => FileType::File,
                }),
            },
            Requirement {
                key: ItemKey::Path(
                    self.link
                        .path()
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("/"))
                        .to_owned()
                        .into(),
                ),
                validator: Validator::FileType(FileType::Directory),
            },
        ]
    }
}

impl<'f> FeatureExt<'f> for features::usergroup::Group<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        vec![Item::Group(Group {
            name: self.name.name().to_owned().into(),
        })]
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        vec![Requirement {
            key: ItemKey::Path(std::path::Path::new("/etc/group").into()),
            validator: Validator::Exists,
        }]
    }
}

impl<'f> FeatureExt<'f> for features::usergroup::User<'f> {
    fn provides(&self) -> Vec<Item<'f>> {
        vec![Item::User(User {
            name: self.name.name().to_owned().into(),
        })]
    }

    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![
            Requirement {
                key: ItemKey::Path(self.home_dir.path().to_owned().into()),
                validator: Validator::FileType(FileType::Directory),
            },
            Requirement {
                key: ItemKey::Path(self.shell.path().to_owned().into()),
                validator: Validator::Executable,
            },
            Requirement {
                key: ItemKey::Path(std::path::Path::new("/etc/passwd").into()),
                validator: Validator::Exists,
            },
            Requirement {
                key: ItemKey::Path(std::path::Path::new("/etc/group").into()),
                validator: Validator::Exists,
            },
        ];
        v.extend(
            self.supplementary_groups
                .iter()
                .chain(vec![&self.primary_group])
                .map(|g| Requirement {
                    key: ItemKey::Group(g.name().to_owned().into()),
                    validator: Validator::Exists,
                }),
        );
        v
    }
}

impl<'f> FeatureExt<'f> for features::usergroup::UserMod<'f> {
    fn requires(&self) -> Vec<Requirement<'f>> {
        let mut v = vec![Requirement {
            key: ItemKey::User(self.username.name().to_owned().into()),
            validator: Validator::Exists,
        }];
        v.extend(self.add_supplementary_groups.iter().map(|g| Requirement {
            key: ItemKey::Group(g.name().to_owned().into()),
            validator: Validator::Exists,
        }));
        v
    }
}
