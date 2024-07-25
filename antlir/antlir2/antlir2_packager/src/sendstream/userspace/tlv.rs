/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::SystemTime;

use uuid::Uuid;

pub(crate) enum Tlv<'a> {
    Uuid(Uuid),
    Ctransid(u64),
    Ino(u64),
    Size(u64),
    Mode(u64),
    Uid(u64),
    Gid(u64),
    Rdev(u64),
    XattrName(&'a [u8]),
    XattrData(&'a [u8]),
    Path(&'a Path),
    PathLink(&'a Path),
    FileOffset(u64),
    Data(&'a [u8]),
    CloneUuid(Uuid),
    CloneCtransid(u64),
    Atime(SystemTime),
    Mtime(SystemTime),
    Ctime(SystemTime),
}

pub(crate) enum TlvData<'a> {
    Bytes(&'a [u8]),
    U64([u8; 8]),
    Timespec(Vec<u8>),
}

impl<'a> AsRef<[u8]> for TlvData<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Bytes(v) => v,
            Self::U64(v) => v.as_ref(),
            Self::Timespec(v) => v,
        }
    }
}

impl Tlv<'_> {
    pub(crate) fn ty(&self) -> u16 {
        match self {
            Self::Uuid(_) => 1,
            Self::Ctransid(_) => 2,
            Self::Ino(_) => 3,
            Self::Size(_) => 4,
            Self::Mode(_) => 5,
            Self::Uid(_) => 6,
            Self::Gid(_) => 7,
            Self::Rdev(_) => 8,
            Self::XattrName(_) => 13,
            Self::XattrData(_) => 14,
            Self::Path(_) => 15,
            Self::PathLink(_) => 17,
            Self::FileOffset(_) => 18,
            Self::Data(_) => 19,
            Self::CloneUuid(_) => 20,
            Self::CloneCtransid(_) => 21,
            Self::Ctime(_) => 9,
            Self::Mtime(_) => 10,
            Self::Atime(_) => 11,
        }
    }

    pub(crate) fn len(&self) -> u16 {
        match self {
            Self::Uuid(_) | Self::CloneUuid(_) => 16,
            Self::Ctransid(_)
            | Self::Ino(_)
            | Self::Size(_)
            | Self::Mode(_)
            | Self::Uid(_)
            | Self::Gid(_)
            | Self::Rdev(_)
            | Self::FileOffset(_)
            | Self::CloneCtransid(_) => 8,
            Self::XattrName(x) | Self::XattrData(x) => x.len() as u16,
            Self::Path(p) | Self::PathLink(p) => p.as_os_str().len() as u16,
            Self::Data(v) => v.len() as u16,
            Self::Atime(_) | Self::Mtime(_) | Self::Ctime(_) => 12, // 64bit sec + 32bit nsec
        }
    }

    pub(crate) fn data(&self) -> TlvData {
        match self {
            Self::Uuid(v) | Self::CloneUuid(v) => TlvData::Bytes(v.as_bytes()),
            Self::Ctransid(v)
            | Self::Ino(v)
            | Self::Size(v)
            | Self::Mode(v)
            | Self::Uid(v)
            | Self::Gid(v)
            | Self::Rdev(v)
            | Self::FileOffset(v)
            | Self::CloneCtransid(v) => TlvData::U64(v.to_le_bytes()),
            Self::XattrName(x) | Self::XattrData(x) => TlvData::Bytes(x),
            Self::Path(p) | Self::PathLink(p) => TlvData::Bytes(p.as_os_str().as_bytes()),
            Self::Data(v) => TlvData::Bytes(v),
            Self::Atime(t) | Self::Mtime(t) | Self::Ctime(t) => {
                let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
                let mut bytes = duration.as_secs().to_le_bytes().to_vec();
                bytes.extend(duration.subsec_nanos().to_le_bytes());
                TlvData::Timespec(bytes)
            }
        }
    }
}
