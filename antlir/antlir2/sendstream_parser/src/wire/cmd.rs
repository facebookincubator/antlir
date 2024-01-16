/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nom::IResult;

use crate::wire::tlv::attr_types;
use crate::wire::tlv::parse_tlv;
use crate::wire::tlv::parse_tlv_with_attr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandHeader {
    /// Command size, excluing command header itself
    pub(crate) len: usize,
    /// Command type, Check btrfs_send_command in kernel send.h for all types
    pub(crate) ty: CommandType,
    /// CRC32 checksum, including the header, with checksum filled with 0.
    pub(crate) crc32: u32,
}

impl CommandHeader {
    pub(crate) fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, len) = nom::number::streaming::le_u32(input)?;
        let (input, ty) = CommandType::parse(input)?;
        let (input, crc32) = nom::number::streaming::le_u32(input)?;
        Ok((
            input,
            Self {
                len: len as usize,
                ty,
                crc32,
            },
        ))
    }
}

macro_rules! command_type {
    ($enm: ident, $($v:ident),+) => {
        /// All of the btrfs sendstream commands. Copied from linux/fs/btrfs/send.h
        /// WARNING: order is important!
        #[derive(
            Debug,
            Copy,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
        )]
        pub(crate) enum $enm {
            $($v,)+
            /// Unknown command, maybe it's new?
            Unknown(u16),
        }

        impl $enm {
            const fn from_u16(u: u16) -> Self {
                match u {
                    $(${index()} => Self::$v,)+
                    _ => Self::Unknown(u),
                }
            }

            #[cfg(test)]
            pub(crate) fn iter() -> impl Iterator<Item = Self> {
                [$(Self::$v,)+].into_iter()
            }
        }
    }
}

command_type!(
    CommandType,
    // variants below
    Unspecified,
    Subvol,
    Snapshot,
    Mkfile,
    Mkdir,
    Mknod,
    Mkfifo,
    Mksock,
    Symlink,
    Rename,
    Link,
    Unlink,
    Rmdir,
    SetXattr,
    RemoveXattr,
    Write,
    Clone,
    Truncate,
    Chmod,
    Chown,
    Utimes,
    End,
    UpdateExtent
);

impl CommandType {
    fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, ty) = nom::number::streaming::le_u16(input)?;
        Ok((input, Self::from_u16(ty)))
    }
}

macro_rules! parse_subtypes {
    ($hdr: expr, $cmd_data:expr, $($t:ident),+) => {
        match $hdr.ty {
            $(CommandType::$t => {
                let (remaining, cmd) = crate::$t::parse($cmd_data).expect(concat!("failed to parse ", stringify!($t)));
                (remaining, cmd.into())
            }),+
            CommandType::End => ($cmd_data, crate::Command::End),
            _ => {
                unreachable!("all btrfs sendstream command types are covered, what is this? {:?}", $hdr)
            }
        }
    }
}

impl<'a> crate::Command<'a> {
    pub(crate) fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, hdr) = CommandHeader::parse(input)?;
        let (input, cmd_data) = nom::bytes::streaming::take(hdr.len)(input)?;
        let (cmd_remaining, cmd): (_, crate::Command) = parse_subtypes!(
            hdr,
            cmd_data,
            Chmod,
            Chown,
            Clone,
            Link,
            Mkdir,
            Mkfifo,
            Mkfile,
            Mknod,
            Mksock,
            RemoveXattr,
            Rename,
            Rmdir,
            SetXattr,
            Snapshot,
            Subvol,
            Symlink,
            Truncate,
            Unlink,
            UpdateExtent,
            Utimes,
            Write
        );

        assert!(cmd_remaining.is_empty(), "command length is wrong",);
        Ok((input, cmd))
    }
}

impl<'a> crate::Subvol<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, uuid) = parse_tlv(input)?;
        let (input, ctransid) = parse_tlv(input)?;
        Ok((
            input,
            Self {
                path,
                uuid,
                ctransid,
            },
        ))
    }
}

impl<'a> crate::Chmod<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, mode) = parse_tlv(input)?;
        Ok((input, Self { path, mode }))
    }
}

impl<'a> crate::Chown<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, uid) = parse_tlv(input)?;
        let (input, gid) = parse_tlv(input)?;
        Ok((input, Self { path, uid, gid }))
    }
}

impl<'a> crate::Clone<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, dst_offset) = parse_tlv(input)?;
        let (input, len) = parse_tlv(input)?;
        let (input, dst_path) = parse_tlv(input)?;
        let (input, uuid) = parse_tlv_with_attr::<_, 16, attr_types::CloneUuid>(input)?;
        let (input, ctransid) = parse_tlv_with_attr::<_, 8, attr_types::CloneCtransid>(input)?;
        let (input, src_path) = parse_tlv_with_attr::<_, 0, attr_types::ClonePath>(input)?;
        let (input, src_offset) = parse_tlv_with_attr::<_, 8, attr_types::CloneOffset>(input)?;
        Ok((
            input,
            Self {
                src_offset,
                len,
                src_path,
                uuid,
                ctransid,
                dst_path,
                dst_offset,
            },
        ))
    }
}

impl<'a> crate::Link<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, link_name) = parse_tlv(input)?;
        let (input, target) = parse_tlv(input)?;
        Ok((input, Self { target, link_name }))
    }
}

impl<'a> crate::Symlink<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, link_name) = parse_tlv(input)?;
        let (input, ino) = parse_tlv(input)?;
        let (input, target) = parse_tlv(input)?;
        Ok((
            input,
            Self {
                target,
                ino,
                link_name,
            },
        ))
    }
}

impl<'a> crate::Mkdir<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, ino) = parse_tlv(input)?;
        Ok((input, Self { path, ino }))
    }
}

impl<'a> crate::Mkfile<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, ino) = parse_tlv(input)?;
        Ok((input, Self { path, ino }))
    }
}

impl<'a> crate::Mkspecial<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, ino) = parse_tlv(input)?;
        let (input, rdev) = parse_tlv(input)?;
        let (input, mode) = parse_tlv(input)?;
        Ok((
            input,
            Self {
                path,
                ino,
                rdev,
                mode,
            },
        ))
    }
}

macro_rules! mkspecial {
    ($t:ident) => {
        impl<'a> crate::$t<'a> {
            fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
                crate::Mkspecial::parse(input).map(|(r, s)| (r, Self(s)))
            }
        }
    };
}

mkspecial!(Mknod);
mkspecial!(Mkfifo);
mkspecial!(Mksock);

impl<'a> crate::RemoveXattr<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, name) = parse_tlv(input)?;
        Ok((input, Self { path, name }))
    }
}

impl<'a> crate::Rename<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, from) = parse_tlv(input)?;
        let (input, to) = parse_tlv_with_attr::<_, 0, attr_types::PathTo>(input)?;
        Ok((input, Self { from, to }))
    }
}

impl<'a> crate::Rmdir<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        Ok((input, Self { path }))
    }
}

impl<'a> crate::SetXattr<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, name) = parse_tlv(input)?;
        let (input, data) = parse_tlv(input)?;
        Ok((input, Self { path, name, data }))
    }
}

impl<'a> crate::Truncate<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, size) = parse_tlv(input)?;
        Ok((input, Self { path, size }))
    }
}

impl<'a> crate::Snapshot<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, uuid) = parse_tlv(input)?;
        let (input, ctransid) = parse_tlv(input)?;
        let (input, clone_uuid) = parse_tlv_with_attr::<_, 16, attr_types::CloneUuid>(input)?;
        let (input, clone_ctransid) =
            parse_tlv_with_attr::<_, 8, attr_types::CloneCtransid>(input)?;
        Ok((
            input,
            Self {
                path,
                uuid,
                ctransid,
                clone_uuid,
                clone_ctransid,
            },
        ))
    }
}

impl<'a> crate::Unlink<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        Ok((input, Self { path }))
    }
}

impl<'a> crate::UpdateExtent<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, offset) = parse_tlv(input)?;
        let (input, len) = parse_tlv(input)?;
        Ok((input, Self { path, offset, len }))
    }
}

impl<'a> crate::Utimes<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, atime) = parse_tlv(input)?;
        let (input, mtime) = parse_tlv(input)?;
        let (input, ctime) = parse_tlv(input)?;
        Ok((
            input,
            Self {
                path,
                atime,
                mtime,
                ctime,
            },
        ))
    }
}

impl<'a> crate::Write<'a> {
    fn parse(input: &'a [u8]) -> IResult<&[u8], Self> {
        let (input, path) = parse_tlv(input)?;
        let (input, offset) = parse_tlv(input)?;
        let (input, data) = parse_tlv(input)?;
        Ok((input, Self { path, offset, data }))
    }
}
