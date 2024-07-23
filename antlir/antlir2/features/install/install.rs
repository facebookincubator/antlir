/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(let_chains)]
use std::collections::HashMap;
use std::fs::File;
use std::fs::FileTimes;
use std::fs::Permissions;
use std::os::unix::fs::fchown;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use antlir2_compile::util::copy_with_metadata;
use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::stat::Mode;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::PathInLayer;
use antlir2_users::GroupId;
use antlir2_users::Id;
use antlir2_users::UserId;
use anyhow::Context;
#[cfg(feature = "setcap")]
use libcap::Capabilities;
#[cfg(feature = "setcap")]
use libcap::FileExt as _;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::Deserialize;
use serde_with::serde_as;
#[cfg(feature = "setcap")]
use serde_with::DisplayFromStr;
use tracing::debug;
use walkdir::WalkDir;
use xattr::FileExt as _;

pub type Feature = Install;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Install {
    pub dst: PathInLayer,
    pub group: NameOrId<GroupId>,
    pub mode: Mode,
    pub src: BuckOutSource,
    pub user: NameOrId<UserId>,
    pub binary_info: Option<BinaryInfo>,
    pub xattrs: HashMap<String, XattrValue>,
    #[cfg(feature = "setcap")]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub setcap: Option<Capabilities>,
    #[cfg(not(feature = "setcap"))]
    pub setcap: Option<String>,
    pub always_use_gnu_debuglink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum NameOrId<I: Id> {
    Name(String),
    Id(I),
}

impl<I: Id> From<&str> for NameOrId<I> {
    fn from(s: &str) -> Self {
        Self::Name(s.to_owned())
    }
}

impl Install {
    pub fn is_dir(&self) -> bool {
        self.src.is_dir()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct SplitBinaryMetadata {
    #[serde(default)]
    pub buildid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BinaryInfo {
    Dev,
    Installed(InstalledBinary),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledBinary {
    pub debuginfo: BuckOutSource,
    pub dwp: Option<BuckOutSource>,
    pub metadata: SplitBinaryMetadata,
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'de> Deserialize<'de> for BinaryInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Deser {
            dev: Option<bool>,
            installed: Option<InstalledBinary>,
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

impl<'de> Deserialize<'de> for InstalledBinary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Deser {
            debuginfo: BuckOutSource,
            dwp: Option<BuckOutSource>,
            metadata: Metadata,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Metadata {
            Metadata(SplitBinaryMetadata),
            Path(BuckOutSource),
        }

        Deser::deserialize(deserializer).and_then(|s| {
            Ok(Self {
                dwp: s.dwp,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XattrValue(Vec<u8>);

impl<'de> Deserialize<'de> for XattrValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(hex_value) = s.strip_prefix("0x") {
            let bytes = hex::decode(hex_value).map_err(D::Error::custom)?;
            Ok(Self(bytes))
        } else if let Some(b64) = s.strip_prefix("0s") {
            let bytes = base64::decode(b64).map_err(D::Error::custom)?;
            Ok(Self(bytes))
        } else {
            Ok(Self(s.into_bytes()))
        }
    }
}

impl antlir2_depgraph_if::RequiresProvides for Install {
    fn provides(&self) -> Result<Vec<Item>, String> {
        if self.is_dir() {
            let mut v = vec![Item::Path(PathItem::Entry(FsEntry {
                path: self.dst.components().collect(),
                file_type: FileType::Directory,
                mode: self.mode.as_raw(),
            }))];
            for entry in WalkDir::new(&self.src) {
                let entry = entry
                    .with_context(|| format!("while walking src dir {}", self.src.display()))
                    .map_err(|e| e.to_string())?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this has to be under src");
                if relpath == Path::new("") {
                    continue;
                }
                if entry.file_type().is_file() {
                    v.push(Item::Path(PathItem::Entry(FsEntry {
                        path: self.dst.join(relpath),
                        file_type: FileType::File,
                        mode: 0o444,
                    })))
                } else if entry.file_type().is_dir() {
                    v.push(Item::Path(PathItem::Entry(FsEntry {
                        path: self.dst.join(relpath),
                        file_type: FileType::Directory,
                        mode: 0o755,
                    })))
                } else if entry.file_type().is_symlink() {
                    let target = std::fs::read_link(entry.path())
                        .with_context(|| {
                            format!("while reading link target of {}", entry.path().display())
                        })
                        .map_err(|e| e.to_string())?;
                    v.push(Item::Path(PathItem::Symlink {
                        link: self.dst.join(relpath),
                        target,
                    }));
                }
            }
            Ok(v)
        } else {
            let mut provides = vec![Item::Path(PathItem::Entry(FsEntry {
                path: self.dst.to_owned(),
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
                        dwp,
                        metadata,
                    }) => {
                        if self.always_use_gnu_debuglink {
                            let debug_dst = self.dst.with_extension("debug");
                            provides.push(Item::Path(PathItem::Entry(FsEntry {
                                path: debug_dst.to_owned(),
                                file_type: FileType::File,
                                mode: 0o444,
                            })));
                            if dwp.is_some() {
                                provides.push(Item::Path(PathItem::Entry(FsEntry {
                                    path: debug_dst.with_extension("debug.dwp"),
                                    file_type: FileType::File,
                                    mode: 0o444,
                                })));
                            }
                        } else if let Some(buildid) = &metadata.buildid {
                            let debug_dst = Path::new("/usr/lib/debug/.build-id")
                                .join(&buildid[..2])
                                .join(&buildid[2..])
                                .with_extension("debug");
                            provides.push(Item::Path(PathItem::Entry(FsEntry {
                                path: debug_dst.parent().expect("must have parent").to_owned(),
                                file_type: FileType::Directory,
                                mode: 0o555,
                            })));
                            provides.push(Item::Path(PathItem::Entry(FsEntry {
                                path: debug_dst.to_owned(),
                                file_type: FileType::File,
                                mode: 0o444,
                            })));
                            if dwp.is_some() {
                                provides.push(Item::Path(PathItem::Entry(FsEntry {
                                    path: debug_dst.with_extension("debug.dwp"),
                                    file_type: FileType::File,
                                    mode: 0o444,
                                })));
                            }
                        }
                    }
                }
            }
            Ok(provides)
        }
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut requires = vec![];
        // if a user gives us the numeric ids, we have to just assume they know
        // what they are doing and intend to bypass the user existence check
        // here
        if let NameOrId::Name(u) = &self.user {
            requires.push(Requirement::ordered(
                ItemKey::User(u.to_owned()),
                Validator::Exists,
            ));
        }
        if let NameOrId::Name(g) = &self.group {
            requires.push(Requirement::ordered(
                ItemKey::Group(g.to_owned()),
                Validator::Exists,
            ));
        }
        // For relative dest paths (or `/`), parent() could be the empty string
        if let Some(parent) = self.dst.parent()
            && !parent.as_os_str().is_empty()
        {
            requires.push(Requirement::ordered(
                ItemKey::Path(parent.to_owned()),
                Validator::FileType(FileType::Directory),
            ));
        }
        Ok(requires)
    }
}

impl antlir2_compile::CompileFeature for Install {
    #[tracing::instrument(name = "install", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let uid = match &self.user {
            NameOrId::Name(n) => ctx.uid(n)?,
            NameOrId::Id(i) => *i,
        };
        let gid = match &self.group {
            NameOrId::Name(n) => ctx.gid(n)?,
            NameOrId::Id(i) => *i,
        };
        if self.is_dir() {
            debug!("{:?} is a dir", self.src);
            for entry in WalkDir::new(&self.src) {
                let entry = entry.map_err(std::io::Error::from)?;
                let relpath = entry
                    .path()
                    .strip_prefix(&self.src)
                    .expect("this must be under src");

                debug!("relpath is {relpath:?}");

                let dst_path = ctx.dst_path(self.dst.join(relpath))?;
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

            let dir_path = ctx.dst_path(&self.dst)?;
            for (key, val) in self.xattrs.iter() {
                xattr::set(&dir_path, key, &val.0)?;
            }
        } else {
            let dst = ctx.dst_path(&self.dst)?;

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
                        std::fs::create_dir_all(ctx.dst_path("/usr/lib/debug")?)?;

                        None
                    }
                    BinaryInfo::Installed(InstalledBinary {
                        dwp,
                        debuginfo,
                        metadata,
                    }) => {
                        if self.always_use_gnu_debuglink {
                            let debug_dst = dst.with_extension("debug");
                            cp_debug_symbols(debuginfo, &debug_dst, dwp, uid, gid)?;
                            copy_with_metadata(
                                &self.src,
                                &dst,
                                Some(uid.as_raw()),
                                Some(gid.as_raw()),
                            )?;
                            add_gnu_debuglink(&dst, &debug_dst)?;
                        } else if let Some(buildid) = &metadata.buildid {
                            let debug_dst = ctx
                                .dst_path(
                                    Path::new("/usr/lib/debug/.build-id")
                                        .join(&buildid[..2])
                                        .join(&buildid[2..]),
                                )?
                                .with_extension("debug");
                            cp_debug_symbols(debuginfo, &debug_dst, dwp, uid, gid)?;
                            copy_with_metadata(
                                &self.src,
                                &dst,
                                Some(uid.as_raw()),
                                Some(gid.as_raw()),
                            )?;
                        } else {
                            copy_with_metadata(
                                &self.src,
                                &dst,
                                Some(uid.as_raw()),
                                Some(gid.as_raw()),
                            )?;
                        };
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
                for (key, val) in &self.xattrs {
                    dst_file.set_xattr(key, &val.0)?;
                }
                if let Some(cap) = self.setcap.as_ref() {
                    // Technically we could just use self.setcap directly, but
                    // then that would still result in a syscall to clear the
                    // file capabilities (which won't even be there)
                    #[cfg(feature = "setcap")]
                    dst_file.set_capabilities(Some(cap))?;
                    #[cfg(not(feature = "setcap"))]
                    return Err(anyhow::anyhow!(
                        "setcap ({cap}) is not supported on this platform"
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}

fn cp_debug_symbols(
    debug_src: &Path,
    debug_dst: &Path,
    maybe_dwp: &Option<BuckOutSource>,
    uid: UserId,
    gid: GroupId,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(
        debug_dst
            .parent()
            .expect("debug_dst will always have parent"),
    )?;

    copy_with_metadata(debug_src, debug_dst, Some(uid.as_raw()), Some(gid.as_raw()))?;
    if let Some(dwp) = maybe_dwp {
        // We want dwps to live alongside the split *.debug file as *.debug.dwp such
        // that it's discoverable by lldb
        copy_with_metadata(
            dwp,
            &debug_dst.with_extension("debug.dwp"),
            Some(uid.as_raw()),
            Some(gid.as_raw()),
        )?;
    }
    Ok(())
}

fn add_gnu_debuglink(src: &Path, debug: &Path) -> anyhow::Result<()> {
    let mut objcopy = std::process::Command::new("objcopy");
    objcopy.arg("--add-gnu-debuglink").arg(debug).arg(src);
    objcopy.status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_xattr() {
        assert_eq!(
            XattrValue(b"foo".to_vec()),
            serde_json::from_str::<XattrValue>(r#""foo""#).expect("failed to deserialize")
        );
        assert_eq!(
            XattrValue(b"bar".to_vec()),
            serde_json::from_str::<XattrValue>(r#""0x626172""#).expect("failed to deserialize")
        );
        assert_eq!(
            XattrValue(b"baz".to_vec()),
            serde_json::from_str::<XattrValue>(r#""0sYmF6""#).expect("failed to deserialize")
        );
    }
}
