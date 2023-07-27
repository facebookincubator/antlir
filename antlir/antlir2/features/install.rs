/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(let_chains)]
use std::borrow::Cow;
use std::fs::File;
use std::fs::FileTimes;
use std::fs::Permissions;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::fchown;
use std::os::unix::fs::PermissionsExt;
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
use antlir2_features::stat::Mode;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::GroupName;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use antlir2_users::Id;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use serde::de::Error;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use walkdir::WalkDir;

pub type Feature = Install<'static>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Install<'a> {
    pub dst: PathInLayer<'a>,
    pub group: GroupName<'a>,
    pub mode: Mode,
    pub src: BuckOutSource<'a>,
    pub user: UserName<'a>,
    pub binary_info: Option<BinaryInfo<'a>>,
}

impl<'a> Install<'a> {
    pub fn is_dir(&self) -> bool {
        self.dst.as_os_str().as_bytes().last().copied() == Some(b'/')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct SplitBinaryMetadata<'a> {
    pub elf: bool,
    #[serde(default)]
    pub buildid: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BinaryInfo<'a> {
    Dev,
    Installed(InstalledBinary<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct InstalledBinary<'a> {
    pub debuginfo: BuckOutSource<'a>,
    pub metadata: SplitBinaryMetadata<'a>,
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'a, 'de: 'a> Deserialize<'de> for BinaryInfo<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct Deser<'a> {
            dev: Option<bool>,
            installed: Option<InstalledBinary<'a>>,
        }

        Deser::deserialize(deserializer).and_then(|s| match (s.dev, s.installed) {
            (Some(true), None) => Ok(Self::Dev),
            (None, Some(installed)) => Ok(Self::Installed(installed)),
            (_, _) => Err(D::Error::custom(
                "exactly one of {dev=True, installed} must be set",
            )),
        })
    }
}

impl<'a> Serialize for BinaryInfo<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Ser<'a, 'b> {
            dev: Option<bool>,
            installed: Option<&'b InstalledBinary<'a>>,
        }
        Ser {
            dev: match self {
                Self::Dev => Some(true),
                _ => None,
            },
            installed: match self {
                Self::Installed(installed) => Some(installed),
                _ => None,
            },
        }
        .serialize(serializer)
    }
}

impl<'a, 'de: 'a> Deserialize<'de> for InstalledBinary<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct Deser<'a> {
            debuginfo: BuckOutSource<'a>,
            metadata: Metadata<'a>,
        }

        #[derive(Deserialize)]
        #[serde(untagged, bound(deserialize = "'de: 'a"))]
        enum Metadata<'a> {
            Metadata(SplitBinaryMetadata<'a>),
            Path(BuckOutSource<'a>),
        }

        Deser::deserialize(deserializer).and_then(|s| {
            Ok(Self {
                debuginfo: s.debuginfo,
                metadata: match s.metadata {
                    Metadata::Path(path) => {
                        let metadata = std::fs::read(path).map_err(D::Error::custom)?;
                        SplitBinaryMetadata::deserialize(
                            &mut serde_json::Deserializer::from_reader(std::io::Cursor::new(
                                metadata,
                            )),
                        )
                        .map_err(D::Error::custom)?
                    }
                    Metadata::Metadata(metadata) => metadata,
                },
            })
        })
    }
}

impl<'f> antlir2_feature_impl::Feature<'f> for Install<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        if self.is_dir() {
            let mut v = vec![Item::Path(PathItem::Entry(FsEntry {
                path: self.dst.path().to_owned().into(),
                file_type: FileType::Directory,
                mode: self.mode.as_raw(),
            }))];
            for entry in WalkDir::new(&self.src) {
                let entry = entry
                    .with_context(|| format!("while walking src dir {}", self.src.display()))?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this has to be under src");
                if relpath == Path::new("") {
                    continue;
                }
                if entry.file_type().is_file() {
                    v.push(Item::Path(PathItem::Entry(FsEntry {
                        path: self.dst.join(relpath).into(),
                        file_type: FileType::File,
                        mode: 0o444,
                    })))
                } else if entry.file_type().is_dir() {
                    v.push(Item::Path(PathItem::Entry(FsEntry {
                        path: self.dst.join(relpath).into(),
                        file_type: FileType::Directory,
                        mode: 0o755,
                    })))
                } else if entry.file_type().is_symlink() {
                    let target = std::fs::read_link(entry.path()).with_context(|| {
                        format!("while reading link target of {}", entry.path().display())
                    })?;
                    v.push(Item::Path(PathItem::Symlink {
                        link: self.dst.join(relpath).into(),
                        target: target.into(),
                    }));
                }
            }
            Ok(v)
        } else {
            let mut provides = vec![Item::Path(PathItem::Entry(FsEntry {
                path: self.dst.path().to_owned().into(),
                file_type: FileType::File,
                mode: self.mode.as_raw(),
            }))];
            if let Some(binary) = &self.binary_info {
                match binary {
                    BinaryInfo::Dev => {
                        provides.push(Item::Path(PathItem::Entry(FsEntry {
                            path: std::path::Path::new("/usr/lib/debug").into(),
                            file_type: FileType::Directory,
                            mode: 0o755,
                        })));
                    }
                    BinaryInfo::Installed(InstalledBinary {
                        debuginfo: _,
                        metadata,
                    }) => {
                        if metadata.elf {
                            let debuginfo_dst = match metadata.buildid.as_ref() {
                                Some(buildid) => Path::new("/usr/lib/debug/.build-id")
                                    .join(&buildid[..2])
                                    .join(&buildid[2..]),
                                None => Path::new("/usr/lib/debug")
                                    .join(self.dst.strip_prefix("/").unwrap_or(&self.dst)),
                            }
                            .with_extension("debug");
                            provides.push(Item::Path(PathItem::Entry(FsEntry {
                                path: debuginfo_dst
                                    .parent()
                                    .expect("must have parent")
                                    .to_owned()
                                    .into(),
                                file_type: FileType::Directory,
                                mode: 0o555,
                            })));
                            provides.push(Item::Path(PathItem::Entry(FsEntry {
                                path: debuginfo_dst.into(),
                                file_type: FileType::File,
                                mode: 0o444,
                            })));
                        }
                    }
                }
            }
            Ok(provides)
        }
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        let mut requires = vec![
            Requirement::ordered(
                ItemKey::User(self.user.name().to_owned().into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Group(self.group.name().to_owned().into()),
                Validator::Exists,
            ),
        ];
        // For relative dest paths (or `/`), parent() could be the empty string
        if let Some(parent) = self.dst.path().parent() && !parent.as_os_str().is_empty() {
            requires.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned().into()),
                Validator::FileType(FileType::Directory),
            ));
        }
        Ok(requires)
    }

    #[tracing::instrument(name = "install", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let uid = ctx.uid(self.user.name())?;
        let gid = ctx.gid(self.group.name())?;
        if self.src.is_dir() {
            debug!("{:?} is a dir", self.src);
            ensure!(
                self.is_dir(),
                "install src ({}) is directory but dst ({}) is missing trailing /",
                self.src.display(),
                self.dst.display()
            );
            for entry in WalkDir::new(&self.src) {
                let entry = entry.map_err(std::io::Error::from)?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this must be under src");

                debug!("relpath is {relpath:?}");

                let dst_path = ctx.dst_path(self.dst.path().join(relpath));
                debug!("dst path is {dst_path:?}");

                // the depgraph already ensured that there are no conflicts, so if
                // this exists then it must have the correct contents
                if dst_path.exists() {
                    tracing::debug!(
                        dst_path = dst_path.display().to_string(),
                        "install destination already exists"
                    );
                    continue;
                }

                copy_with_metadata(
                    entry.path(),
                    &dst_path,
                    Some(uid.as_raw()),
                    Some(gid.as_raw()),
                )?;
            }
        } else {
            ensure!(
                !self.is_dir(),
                "install dst ({}) is claiming to be directory but src ({}) is a file",
                self.dst.display(),
                self.src.display()
            );
            let dst = ctx.dst_path(&self.dst);

            let dst_file = match &self.binary_info {
                Some(binary_info) => match binary_info {
                    BinaryInfo::Dev => {
                        // If we are installing a buck-built binary in @mode/dev, it must be
                        // executed from the exact same path so that it can find relatively
                        // located .so libraries. There are two ways to do this:
                        // 1) make a symlink to the binary
                        // 2) install a shell script that `exec`s the real binary at the right
                        // path
                        //
                        // Antlir2 chooses option 1, since it's substantially simpler and does
                        // not require any assumptions about the layer (like /bin/sh even
                        // existing).
                        let src_abspath = std::fs::canonicalize(&self.src)?;
                        std::os::unix::fs::symlink(src_abspath, &dst)?;

                        // TODO(vmagro): figure out how to kill this - it exists
                        // only so that /usr/lib/debug can be unconditionally
                        // cloned out of the source layer, but this feels dirty
                        std::fs::create_dir_all(ctx.dst_path("/usr/lib/debug"))?;

                        None
                    }
                    BinaryInfo::Installed(InstalledBinary {
                        debuginfo,
                        metadata,
                    }) => {
                        if metadata.elf {
                            let debuginfo_dst = ctx
                                .dst_path(match metadata.buildid.as_ref() {
                                    Some(buildid) => Path::new("/usr/lib/debug/.build-id")
                                        .join(&buildid[..2])
                                        .join(&buildid[2..]),
                                    None => Path::new("/usr/lib/debug")
                                        .join(self.dst.strip_prefix("/").unwrap_or(&self.dst)),
                                })
                                .with_extension("debug");
                            std::fs::create_dir_all(
                                debuginfo_dst
                                    .parent()
                                    .expect("debuginfo_dst will always have parent"),
                            )?;
                            copy_with_metadata(
                                debuginfo,
                                &debuginfo_dst,
                                Some(uid.as_raw()),
                                Some(gid.as_raw()),
                            )?;
                        }
                        copy_with_metadata(
                            &self.src,
                            &dst,
                            Some(uid.as_raw()),
                            Some(gid.as_raw()),
                        )?;
                        let dst_file = File::options().write(true).open(&dst)?;
                        Some(dst_file)
                    }
                },
                None => {
                    std::fs::copy(&self.src, &dst)?;
                    let dst_file = File::options().write(true).open(&dst)?;
                    Some(dst_file)
                }
            };

            if let Some(dst_file) = dst_file {
                fchown(&dst_file, Some(uid.into()), Some(gid.into()))
                    .map_err(std::io::Error::from)?;
                dst_file.set_permissions(Permissions::from_mode(self.mode.as_raw()))?;

                // Sync the file times with the source. This is not strictly necessary
                // but does lead to some better reproducibility of image builds as it's
                // one less entropic thing to change between runs when the input did not
                // change
                let src_meta = std::fs::metadata(&self.src)?;
                let times = FileTimes::new()
                    .set_accessed(src_meta.accessed()?)
                    .set_modified(src_meta.modified()?);
                dst_file.set_times(times)?;
            }
        }
        Ok(())
    }
}
