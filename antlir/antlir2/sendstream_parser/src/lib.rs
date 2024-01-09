/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Rust parser for [BTRFS
//! Sendstreams](https://btrfs.readthedocs.io/en/latest/Send-receive.html)
//! which are created via
//! [btrfs-send](https://btrfs.readthedocs.io/en/latest/btrfs-send.html).

#![feature(macro_metavar_expr)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use std::borrow::Cow;
use std::ops::Deref;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;

use derive_more::AsRef;
use derive_more::Deref;
use derive_more::From;
use nix::sys::stat::SFlag;
use nix::unistd::Gid;
use nix::unistd::Uid;
#[cfg(feature = "serde")]
use serde::Deserialize;
#[cfg(feature = "serde")]
use serde::Serialize;
use uuid::Uuid;

#[cfg(feature = "serde")]
mod ser;
mod wire;

#[derive(Debug, thiserror::Error)]
pub enum Error<'a> {
    #[error("Parse error: {0:?}")]
    Parse(nom::error::Error<&'a [u8]>),
    #[error(
        "Sendstream had unexpected trailing data. This probably means the parser is broken: {0:?}"
    )]
    TrailingData(Vec<u8>),
    #[error("Sendstream is incomplete")]
    Incomplete,
}

impl<'a> From<nom::error::Error<&'a [u8]>> for Error<'a> {
    fn from(e: nom::error::Error<&'a [u8]>) -> Self {
        Self::Parse(e)
    }
}

pub type Result<'a, R> = std::result::Result<R, Error<'a>>;

/// This is the main entrypoint of this crate. It provides access to the
/// sequence of [Command]s that make up this sendstream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Sendstream<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    commands: Vec<Command<'a>>,
}

impl<'a> Sendstream<'a> {
    pub fn commands(&self) -> &[Command<'a>] {
        &self.commands
    }

    pub fn into_commands(self) -> Vec<Command<'a>> {
        self.commands
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(bound(deserialize = "'de: 'a")))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Command<'a> {
    Chmod(Chmod<'a>),
    Chown(Chown<'a>),
    Clone(Clone<'a>),
    End,
    Link(Link<'a>),
    Mkdir(Mkdir<'a>),
    Mkfifo(Mkfifo<'a>),
    Mkfile(Mkfile<'a>),
    Mknod(Mknod<'a>),
    Mksock(Mksock<'a>),
    RemoveXattr(RemoveXattr<'a>),
    Rename(Rename<'a>),
    Rmdir(Rmdir<'a>),
    SetXattr(SetXattr<'a>),
    Snapshot(Snapshot<'a>),
    Subvol(Subvol<'a>),
    Symlink(Symlink<'a>),
    Truncate(Truncate<'a>),
    Unlink(Unlink<'a>),
    UpdateExtent(UpdateExtent<'a>),
    Utimes(Utimes<'a>),
    Write(Write<'a>),
}

impl<'a> Command<'a> {
    /// Exposed for tests to ensure that the demo sendstream is exhaustive and
    /// exercises all commands
    #[cfg(test)]
    pub(crate) fn command_type(&self) -> wire::cmd::CommandType {
        match self {
            Self::Chmod(_) => wire::cmd::CommandType::Chmod,
            Self::Chown(_) => wire::cmd::CommandType::Chown,
            Self::Clone(_) => wire::cmd::CommandType::Clone,
            Self::End => wire::cmd::CommandType::End,
            Self::Link(_) => wire::cmd::CommandType::Link,
            Self::Mkdir(_) => wire::cmd::CommandType::Mkdir,
            Self::Mkfifo(_) => wire::cmd::CommandType::Mkfifo,
            Self::Mkfile(_) => wire::cmd::CommandType::Mkfile,
            Self::Mknod(_) => wire::cmd::CommandType::Mknod,
            Self::Mksock(_) => wire::cmd::CommandType::Mksock,
            Self::RemoveXattr(_) => wire::cmd::CommandType::RemoveXattr,
            Self::Rename(_) => wire::cmd::CommandType::Rename,
            Self::Rmdir(_) => wire::cmd::CommandType::Rmdir,
            Self::SetXattr(_) => wire::cmd::CommandType::SetXattr,
            Self::Snapshot(_) => wire::cmd::CommandType::Snapshot,
            Self::Subvol(_) => wire::cmd::CommandType::Subvol,
            Self::Symlink(_) => wire::cmd::CommandType::Symlink,
            Self::Truncate(_) => wire::cmd::CommandType::Truncate,
            Self::Unlink(_) => wire::cmd::CommandType::Unlink,
            Self::UpdateExtent(_) => wire::cmd::CommandType::UpdateExtent,
            Self::Utimes(_) => wire::cmd::CommandType::Utimes,
            Self::Write(_) => wire::cmd::CommandType::Write,
        }
    }
}

macro_rules! from_cmd {
    ($t:ident) => {
        impl<'a> From<$t<'a>> for Command<'a> {
            fn from(c: $t<'a>) -> Self {
                Self::$t(c)
            }
        }
    };
}

macro_rules! one_getter {
    ($f:ident, $ft:ty, copy) => {
        pub fn $f(&self) -> $ft {
            self.$f
        }
    };
    ($f:ident, $ft:ty, borrow) => {
        pub fn $f(&self) -> &$ft {
            &self.$f
        }
    };
}

macro_rules! getters {
    ($t:ident, [$(($f:ident, $ft:ident, $ref:tt)),+]) => {
        impl<'a> $t<'a> {
            $(
                one_getter!($f, $ft, $ref);
            )+
        }
    };
}

/// Because the stream is emitted in inode order, not FS order, the destination
/// directory may not exist at the time that a creation command is emitted, so
/// it will end up with an opaque name that will end up getting renamed to the
/// final name later in the stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef)]
#[as_ref(forward)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct TemporaryPath<'a>(#[cfg_attr(feature = "serde", serde(borrow))] pub(crate) &'a Path);

impl<'a> TemporaryPath<'a> {
    pub fn as_path(&self) -> &Path {
        self.as_ref()
    }
}

impl<'a> Deref for TemporaryPath<'a> {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Ctransid(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Subvol<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) uuid: Uuid,
    pub(crate) ctransid: Ctransid,
}
from_cmd!(Subvol);
getters! {Subvol, [(path, Path, borrow), (uuid, Uuid, copy), (ctransid, Ctransid, copy)]}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Mode(u32);

impl Mode {
    pub fn mode(self) -> nix::sys::stat::Mode {
        nix::sys::stat::Mode::from_bits_truncate(self.0)
    }

    pub fn permissions(self) -> std::fs::Permissions {
        std::fs::Permissions::from_mode(self.0)
    }

    pub fn file_type(self) -> SFlag {
        SFlag::from_bits_truncate(self.0)
    }
}

impl std::fmt::Debug for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mode")
            .field("permissions", &self.permissions())
            .field("type", &self.file_type())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Chmod<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) mode: Mode,
}
from_cmd!(Chmod);
getters! {Chmod, [(path, Path, borrow), (mode, Mode, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Chown<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    #[cfg_attr(feature = "serde", serde(with = "crate::ser::uid"))]
    pub(crate) uid: Uid,
    #[cfg_attr(feature = "serde", serde(with = "crate::ser::gid"))]
    pub(crate) gid: Gid,
}
from_cmd!(Chown);
getters! {Chown, [(path, Path, borrow), (uid, Uid, copy), (gid, Gid, copy)]}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct CloneLen(u64);

impl CloneLen {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Clone<'a> {
    pub(crate) src_offset: FileOffset,
    pub(crate) len: CloneLen,
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) src_path: &'a Path,
    pub(crate) uuid: Uuid,
    pub(crate) ctransid: Ctransid,
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) dst_path: &'a Path,
    pub(crate) dst_offset: FileOffset,
}
from_cmd!(Clone);
getters! {Clone, [
    (src_offset, FileOffset, copy),
    (len, CloneLen, copy),
    (src_path, Path, borrow),
    (uuid, Uuid, copy),
    (ctransid, Ctransid, copy),
    (dst_path, Path, borrow),
    (dst_offset, FileOffset, copy)
]}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef)]
#[as_ref(forward)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct LinkTarget<'a>(#[cfg_attr(feature = "serde", serde(borrow))] &'a Path);

impl<'a> LinkTarget<'a> {
    #[inline]
    pub fn as_path(&self) -> &Path {
        self.0
    }
}

impl<'a> Deref for LinkTarget<'a> {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Link<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) link_name: &'a Path,
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) target: LinkTarget<'a>,
}
from_cmd!(Link);
getters! {Link, [(link_name, Path, borrow), (target, LinkTarget, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Mkdir<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: TemporaryPath<'a>,
    pub(crate) ino: Ino,
}
from_cmd!(Mkdir);
getters! {Mkdir, [(path, TemporaryPath, borrow), (ino, Ino, copy)]}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Rdev(u64);

impl Rdev {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Mkspecial<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: TemporaryPath<'a>,
    pub(crate) ino: Ino,
    pub(crate) rdev: Rdev,
    pub(crate) mode: Mode,
}
getters! {Mkspecial, [
    (path, TemporaryPath, borrow),
    (ino, Ino, copy),
    (rdev, Rdev, copy),
    (mode, Mode, copy)
]}

macro_rules! special {
    ($t:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, AsRef, Deref)]
        #[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[repr(transparent)]
        pub struct $t<'a>(#[cfg_attr(feature = "serde", serde(borrow))] Mkspecial<'a>);
        from_cmd!($t);
    };
}
special!(Mkfifo);
special!(Mknod);
special!(Mksock);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Mkfile<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: TemporaryPath<'a>,
    pub(crate) ino: Ino,
}
from_cmd!(Mkfile);
getters! {Mkfile, [(path, TemporaryPath, borrow), (ino, Ino, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct RemoveXattr<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) name: XattrName<'a>,
}
from_cmd!(RemoveXattr);
getters! {RemoveXattr, [(path, Path, borrow), (name, XattrName, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Rename<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) from: &'a Path,
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) to: &'a Path,
}
from_cmd!(Rename);
getters! {Rename, [(from, Path, borrow), (to, Path, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Rmdir<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
}
from_cmd!(Rmdir);
getters! {Rmdir, [(path, Path, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Symlink<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) link_name: &'a Path,
    pub(crate) ino: Ino,
    pub(crate) target: LinkTarget<'a>,
}
from_cmd!(Symlink);
getters! {Symlink, [(link_name, Path, borrow), (ino, Ino, copy), (target, LinkTarget, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, From)]
#[as_ref(forward)]
#[from(forward)]
pub struct XattrName<'a>(&'a [u8]);

impl<'a> XattrName<'a> {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.0
    }
}

impl<'a> Deref for XattrName<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, AsRef, From)]
#[as_ref(forward)]
#[from(forward)]
pub struct XattrData<'a>(&'a [u8]);

impl<'a> XattrData<'a> {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.0
    }
}

impl<'a> Deref for XattrData<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct SetXattr<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) name: XattrName<'a>,
    pub(crate) data: XattrData<'a>,
}
from_cmd!(SetXattr);
getters! {SetXattr, [(path, Path, borrow), (name, XattrName, borrow), (data, XattrData, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Snapshot<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) uuid: Uuid,
    pub(crate) ctransid: Ctransid,
    pub(crate) clone_uuid: Uuid,
    pub(crate) clone_ctransid: Ctransid,
}
from_cmd!(Snapshot);
getters! {Snapshot, [
    (path, Path, borrow),
    (uuid, Uuid, copy),
    (ctransid, Ctransid, copy),
    (clone_uuid, Uuid, copy),
    (clone_ctransid, Ctransid, copy)
]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Truncate<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) size: u64,
}
from_cmd!(Truncate);
getters! {Truncate, [(path, Path, borrow), (size, u64, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Unlink<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
}
from_cmd!(Unlink);
getters! {Unlink, [(path, Path, borrow)]}

#[allow(clippy::len_without_is_empty)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct UpdateExtent<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) offset: FileOffset,
    pub(crate) len: u64,
}
from_cmd!(UpdateExtent);
getters! {UpdateExtent, [(path, Path, borrow), (offset, FileOffset, copy), (len, u64, copy)]}

macro_rules! time_alias {
    ($a:ident) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
        #[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[as_ref(forward)]
        #[repr(transparent)]
        pub struct $a(std::time::SystemTime);
    };
}

time_alias!(Atime);
time_alias!(Ctime);
time_alias!(Mtime);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Utimes<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) atime: Atime,
    pub(crate) mtime: Mtime,
    pub(crate) ctime: Ctime,
}
from_cmd!(Utimes);
getters! {Utimes, [(path, Path, borrow), (atime, Atime, copy), (mtime, Mtime,copy), (ctime, Ctime, copy)]}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Ino(u64);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct FileOffset(u64);

impl FileOffset {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, From)]
#[as_ref(forward)]
pub struct Data<'a>(&'a [u8]);

impl<'a> Data<'a> {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.0
    }
}

impl<'a> Deref for Data<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> std::fmt::Debug for Data<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match std::str::from_utf8(self.0) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(hex::encode(self.0)),
        };
        if s.len() <= 128 {
            write!(f, "{s:?}")
        } else {
            write!(
                f,
                "{:?} <truncated ({}b total)> {:?}",
                &s[..64],
                s.len(),
                &s[s.len() - 64..]
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Write<'a> {
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub(crate) path: &'a Path,
    pub(crate) offset: FileOffset,
    pub(crate) data: Data<'a>,
}
from_cmd!(Write);
getters! {Write, [(path, Path, borrow), (offset, FileOffset, copy), (data, Data, borrow)]}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::fmt::Write;

    use similar_asserts::SimpleDiff;

    use super::*;

    // serialize sendstream commands to diffable text
    fn serialize_to_txt(sendstreams: &[Sendstream]) -> String {
        let mut out = String::new();
        for (i, s) in sendstreams.iter().enumerate() {
            writeln!(out, "BEGIN SENDSTREAM {i}").unwrap();
            for cmd in s.commands() {
                writeln!(out, "{cmd:?}").unwrap();
            }
            writeln!(out, "END SENDSTREAM {i}").unwrap();
        }
        out
    }

    #[test]
    fn parse_demo() {
        let sendstreams = Sendstream::parse_all(include_bytes!("../testdata/demo.sendstream"))
            .expect("failed to parse demo.sendstream");
        let parsed_txt = serialize_to_txt(&sendstreams);
        if let Some(dst) = std::env::var_os("UPDATE_DEMO_TXT") {
            std::fs::write(dst, serialize_to_txt(&sendstreams)).unwrap();
        } else {
            let good_txt = include_str!("../testdata/demo.txt");
            if parsed_txt != good_txt {
                panic!(
                    "{}",
                    SimpleDiff::from_str(&parsed_txt, good_txt, "parsed", "good")
                )
            }
        }
    }

    #[test]
    fn sendstream_covers_all_commands() {
        let all_cmds: BTreeSet<_> = wire::cmd::CommandType::iter()
            .filter(|c| *c != wire::cmd::CommandType::Unspecified)
            // update_extent is used for no-file-data sendstreams (`btrfs send
            // --no-data`), so it's not super useful to cover here
            .filter(|c| *c != wire::cmd::CommandType::UpdateExtent)
            .collect();
        let sendstreams = Sendstream::parse_all(include_bytes!("../testdata/demo.sendstream"))
            .expect("failed to parse demo.sendstream");
        let seen_cmds = sendstreams
            .iter()
            .flat_map(|s| s.commands.iter().map(|c| c.command_type()))
            .collect();

        if all_cmds != seen_cmds {
            let missing: BTreeSet<_> = all_cmds.difference(&seen_cmds).collect();
            panic!("sendstream did not include some commands: {:?}", missing,);
        }
    }
}
