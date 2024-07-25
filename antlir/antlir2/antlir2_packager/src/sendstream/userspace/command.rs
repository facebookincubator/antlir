/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::time::SystemTime;

use uuid::Uuid;

use super::tlv::Tlv;
use super::writer::CommandBuilder;

pub(crate) fn end() -> Vec<u8> {
    CommandBuilder::new(21).finish()
}

pub(crate) fn subvol<P>(path: P, uuid: Uuid, ctransid: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(1)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Uuid(uuid))
        .tlv(&Tlv::Ctransid(ctransid))
        .finish()
}

pub(crate) fn chown<P>(path: P, uid: u64, gid: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(19)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Uid(uid))
        .tlv(&Tlv::Gid(gid))
        .finish()
}

pub(crate) fn chmod<P>(path: P, mode: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(18)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Mode(mode))
        .finish()
}

pub(crate) fn mkfile<P>(path: P, ino: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(3)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Ino(ino))
        .finish()
}

pub(crate) fn mkdir<P>(path: P, ino: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(4)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Ino(ino))
        .finish()
}

pub(crate) fn symlink<P1, P2>(original: P1, link: P2, ino: u64) -> Vec<u8>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    CommandBuilder::new(8)
        .tlv(&Tlv::Path(link.as_ref()))
        .tlv(&Tlv::Ino(ino))
        .tlv(&Tlv::PathLink(original.as_ref()))
        .finish()
}

pub(crate) fn write<P>(path: P, offset: u64, data: &[u8]) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(15)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::FileOffset(offset))
        .tlv(&Tlv::Data(data))
        .finish()
}

pub(crate) fn hardlink<P1, P2>(original: P1, link: P2) -> Vec<u8>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    CommandBuilder::new(10)
        .tlv(&Tlv::Path(link.as_ref()))
        .tlv(&Tlv::PathLink(original.as_ref()))
        .finish()
}

pub(crate) fn mknod<P>(path: P, mode: u64, rdev: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(5)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Mode(mode))
        .tlv(&Tlv::Rdev(rdev))
        .finish()
}

pub(crate) fn mkfifo<P>(path: P, ino: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(6)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Ino(ino))
        .finish()
}

pub(crate) fn mksock<P>(path: P, ino: u64) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(7)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Ino(ino))
        .finish()
}

pub(crate) fn set_xattr<P, N, V>(path: P, name: N, data: V) -> Vec<u8>
where
    P: AsRef<Path>,
    N: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    CommandBuilder::new(13)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::XattrName(name.as_ref()))
        .tlv(&Tlv::XattrData(data.as_ref()))
        .finish()
}

pub(crate) fn utimes<P>(path: P, atime: SystemTime, mtime: SystemTime, ctime: SystemTime) -> Vec<u8>
where
    P: AsRef<Path>,
{
    CommandBuilder::new(20)
        .tlv(&Tlv::Path(path.as_ref()))
        .tlv(&Tlv::Atime(atime))
        .tlv(&Tlv::Mtime(mtime))
        .tlv(&Tlv::Ctime(ctime))
        .finish()
}
