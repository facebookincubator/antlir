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
use std::ffi::OsStr;
use std::ops::Deref;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;

use bytes::Bytes;
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
pub mod wire;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "Sendstream had unexpected trailing data ({0} bytes). This probably means the parser is broken"
    )]
    TrailingData(usize),
    #[error("Sendstream is incomplete")]
    Incomplete,
    #[error("IO error: {0:#}")]
    IO(#[from] std::io::Error),
    #[error("Sendstream contains unparsable bytes: {0}")]
    Unparsable(String),
}

pub type Result<R> = std::result::Result<R, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Command {
    Unknown(Unknown),
    Chmod(Chmod),
    Chown(Chown),
    Clone(Clone),
    End,
    Link(Link),
    Mkdir(Mkdir),
    Mkfifo(Mkfifo),
    Mkfile(Mkfile),
    Mknod(Mknod),
    Mksock(Mksock),
    RemoveXattr(RemoveXattr),
    Rename(Rename),
    Rmdir(Rmdir),
    SetXattr(SetXattr),
    Snapshot(Snapshot),
    Subvol(Subvol),
    Symlink(Symlink),
    Truncate(Truncate),
    Unlink(Unlink),
    UpdateExtent(UpdateExtent),
    Utimes(Utimes),
    Write(Write),
    EncodedWrite(EncodedWrite),
}

impl Command {
    /// Exposed for tests to ensure that the demo sendstream is exhaustive and
    /// exercises all commands
    #[cfg(test)]
    pub(crate) fn command_type(&self) -> wire::cmd::CommandType {
        match self {
            Self::Unknown(u) => u.command_type,
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
            Self::EncodedWrite(_) => wire::cmd::CommandType::EncodedWrite,
        }
    }
}

macro_rules! from_cmd {
    ($t:ident) => {
        impl From<$t> for Command {
            fn from(c: $t) -> Self {
                Self::$t(c)
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef)]
#[as_ref(forward)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Unknown {
    command_type: wire::cmd::CommandType,
}
from_cmd!(Unknown);

macro_rules! one_getter {
    ($f:ident, $ft:ty, copy) => {
        pub fn $f(&self) -> $ft {
            self.$f
        }
    };
    ($f:ident, Path, borrow) => {
        pub fn $f(&self) -> &Path {
            self.$f.as_ref()
        }
    };
    ($f:ident, $ft:ty, borrow) => {
        pub fn $f(&self) -> &$ft {
            &self.$f
        }
    };
}

macro_rules! getters {
    ($t:ident, [$(($f:ident, $ft:ty, $ref:tt)),+]) => {
        impl $t {
            $(
                one_getter!($f, $ft, $ref);
            )+
        }
    };
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct BytesPath(Bytes);

impl AsRef<Path> for BytesPath {
    fn as_ref(&self) -> &Path {
        Path::new(OsStr::from_bytes(&self.0))
    }
}

impl Deref for BytesPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl std::fmt::Debug for BytesPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path: &Path = self.as_ref();
        path.fmt(f)
    }
}

/// Because the stream is emitted in inode order, not FS order, the destination
/// directory may not exist at the time that a creation command is emitted, so
/// it will end up with an opaque name that will end up getting renamed to the
/// final name later in the stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef)]
#[as_ref(forward)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct TemporaryPath(pub(crate) BytesPath);

impl TemporaryPath {
    pub fn as_path(&self) -> &Path {
        self.0.as_ref()
    }
}

impl Deref for TemporaryPath {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Ctransid(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Subvol {
    pub(crate) path: BytesPath,
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
pub struct Chmod {
    pub(crate) path: BytesPath,
    pub(crate) mode: Mode,
}
from_cmd!(Chmod);
getters! {Chmod, [(path, Path, borrow), (mode, Mode, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Chown {
    pub(crate) path: BytesPath,
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
pub struct Clone {
    pub(crate) src_offset: FileOffset,
    pub(crate) len: CloneLen,
    pub(crate) src_path: BytesPath,
    pub(crate) uuid: Uuid,
    pub(crate) ctransid: Ctransid,
    pub(crate) dst_path: BytesPath,
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
pub struct LinkTarget(BytesPath);

impl LinkTarget {
    #[inline]
    pub fn as_path(&self) -> &Path {
        self.0.as_ref()
    }
}

impl Deref for LinkTarget {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Link {
    pub(crate) link_name: BytesPath,
    pub(crate) target: LinkTarget,
}
from_cmd!(Link);
getters! {Link, [(link_name, BytesPath, borrow), (target, LinkTarget, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Mkdir {
    pub(crate) path: TemporaryPath,
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
pub struct Mkspecial {
    pub(crate) path: TemporaryPath,
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
        pub struct $t(Mkspecial);
        from_cmd!($t);
    };
}
special!(Mkfifo);
special!(Mknod);
special!(Mksock);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Mkfile {
    pub(crate) path: TemporaryPath,
    pub(crate) ino: Ino,
}
from_cmd!(Mkfile);
getters! {Mkfile, [(path, TemporaryPath, borrow), (ino, Ino, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct RemoveXattr {
    pub(crate) path: BytesPath,
    pub(crate) name: XattrName,
}
from_cmd!(RemoveXattr);
getters! {RemoveXattr, [(path, Path, borrow), (name, XattrName, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Rename {
    pub(crate) from: BytesPath,
    pub(crate) to: BytesPath,
}
from_cmd!(Rename);
getters! {Rename, [(from, Path, borrow), (to, Path, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Rmdir {
    pub(crate) path: BytesPath,
}
from_cmd!(Rmdir);
getters! {Rmdir, [(path, Path, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Symlink {
    pub(crate) link_name: BytesPath,
    pub(crate) ino: Ino,
    pub(crate) target: LinkTarget,
}
from_cmd!(Symlink);
getters! {Symlink, [(link_name, Path, borrow), (ino, Ino, copy), (target, LinkTarget, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, From)]
#[as_ref(forward)]
#[from(forward)]
pub struct XattrName(Bytes);

impl XattrName {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for XattrName {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, AsRef, From)]
#[as_ref(forward)]
#[from(forward)]
pub struct XattrData(Bytes);

impl XattrData {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for XattrData {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct SetXattr {
    pub(crate) path: BytesPath,
    pub(crate) name: XattrName,
    pub(crate) data: XattrData,
}
from_cmd!(SetXattr);
getters! {SetXattr, [(path, Path, borrow), (name, XattrName, borrow), (data, XattrData, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Snapshot {
    pub(crate) path: BytesPath,
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
pub struct Truncate {
    pub(crate) path: BytesPath,
    pub(crate) size: u64,
}
from_cmd!(Truncate);
getters! {Truncate, [(path, Path, borrow), (size, u64, copy)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Unlink {
    pub(crate) path: BytesPath,
}
from_cmd!(Unlink);
getters! {Unlink, [(path, Path, borrow)]}

#[allow(clippy::len_without_is_empty)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct UpdateExtent {
    pub(crate) path: BytesPath,
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
time_alias!(Otime);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Utimes {
    pub(crate) path: BytesPath,
    pub(crate) atime: Atime,
    pub(crate) mtime: Mtime,
    pub(crate) ctime: Ctime,
    pub(crate) otime: Option<Otime>,
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
pub struct Data(pub(crate) Bytes);

impl Data {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for Data {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl std::fmt::Debug for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match std::str::from_utf8(&self.0) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(hex::encode(&self.0)),
        };
        if s.chars().count() <= 128 {
            write!(f, "{s:?}")
        } else {
            let left: String = s.chars().take(64).collect();
            let right: String = s
                .chars()
                .rev()
                .take(64)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            write!(f, "{left:?} <truncated ({}b total)> {right:?}", s.len(),)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct Write {
    pub(crate) path: BytesPath,
    pub(crate) offset: FileOffset,
    pub(crate) data: Data,
}
from_cmd!(Write);
getters! {Write, [(path, Path, borrow), (offset, FileOffset, copy), (data, Data, borrow)]}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct EncodedWrite {
    pub(crate) path: BytesPath,
    pub(crate) offset: FileOffset,
    pub(crate) unencoded_file_len: UnencodedFileLen,
    pub(crate) unencoded_len: UnencodedLen,
    pub(crate) unencoded_offset: UnencodedOffset,
    pub(crate) compression: Compression,
    pub(crate) encryption: Option<Encryption>,
    pub(crate) data: Data,
}
from_cmd!(EncodedWrite);
getters! {EncodedWrite, [
    (path, Path, borrow),
    (offset, FileOffset, copy),
    (unencoded_file_len, UnencodedFileLen, copy),
    (unencoded_offset, UnencodedOffset, copy),
    (compression, Compression, copy),
    (encryption, Option<Encryption>, copy),
    (data, Data, borrow)
]}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UnencodedFileLen(u64);

impl UnencodedFileLen {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UnencodedLen(u64);

impl UnencodedLen {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UnencodedOffset(u64);

impl UnencodedOffset {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Compression(u32);

impl Compression {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Encryption(u32);

impl Encryption {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct End;

impl From<End> for Command {
    fn from(_: End) -> Self {
        Self::End
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fmt::Write;
    use std::io::Cursor;

    use futures::StreamExt;
    use futures::future;
    use similar_asserts::SimpleDiff;

    use super::*;

    fn serialize_cmd(idx: &mut u64, out: &mut String, cmd: &Command) {
        match cmd {
            Command::Subvol(_) | Command::Snapshot(_) => {
                writeln!(out, "BEGIN SENDSTREAM {idx}").expect("while writing");
                writeln!(out, "{cmd:?}").expect("while writing");
            }
            Command::End => {
                writeln!(out, "{cmd:?}").expect("while writing");
                writeln!(out, "END SENDSTREAM {idx}").expect("while writing");
                *idx += 1;
            }
            _ => {
                writeln!(out, "{cmd:?}").expect("while writing");
            }
        }
    }

    #[tokio::test]
    async fn parse_demo() {
        let data = include_bytes!("../testdata/demo.sendstream");
        let cursor = Cursor::new(data);
        let mut parsed_txt = String::new();
        let mut sendstream_index = 0;
        let mut num_cmds_parsed = 0;
        let mut parser = wire::parse(cursor);
        while let Some(cmd_res) = parser.next().await {
            let cmd = cmd_res.expect("failed to parse");
            serialize_cmd(&mut sendstream_index, &mut parsed_txt, &cmd);
            num_cmds_parsed += 1;
        }
        if let Some(dst) = std::env::var_os("UPDATE_DEMO_TXT") {
            std::fs::write(dst, parsed_txt).expect("while writing to {dst}");
        } else {
            let good_txt = include_str!("../testdata/demo.txt");
            if parsed_txt != good_txt {
                panic!(
                    "{}",
                    SimpleDiff::from_str(good_txt, &parsed_txt, "good", "parsed")
                )
            }
        }
        assert_eq!(num_cmds_parsed, 106);
    }

    #[tokio::test]
    async fn sendstream_covers_all_commands() {
        let all_cmds: BTreeSet<_> = wire::cmd::PARSED_SUBTYPES
            .iter()
            .copied()
            // update_extent is used for no-file-data sendstreams (`btrfs send
            // --no-data`), so it's not super useful to cover here
            .filter(|c| *c != wire::cmd::CommandType::UpdateExtent)
            .collect();
        let data = include_bytes!("../testdata/demo.sendstream");
        let cursor = Cursor::new(data);
        let seen_cmds: BTreeSet<_> = wire::parse(cursor)
            .filter_map(|i| {
                future::ready(match i {
                    Ok(cmd) => Some(cmd.command_type()),
                    Err(e) => panic!("failed to parse: {e}"),
                })
            })
            .collect()
            .await;
        assert_eq!(seen_cmds, all_cmds);
    }
}
