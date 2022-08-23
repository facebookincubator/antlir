#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Parses the btrfs send-stream binary format. Only version 1 is supported."
import enum
import os
import struct
import uuid
from io import BytesIO
from typing import Iterator, NamedTuple, Tuple

from antlir.btrfs_diff.send_stream import SendStreamItem, SendStreamItems


BTRFS_SEND_STREAM_MAGIC = b"btrfs-stream\0"


def file_unpack(fmt, infile):
    size = struct.calcsize(fmt)
    b = infile.read(size)
    if len(b) != size:
        raise RuntimeError(f"Not enough bytes {b} for format {fmt}")
    return struct.unpack(fmt, b)


def check_magic(infile) -> None:
    magic = infile.read(len(BTRFS_SEND_STREAM_MAGIC))
    if magic != BTRFS_SEND_STREAM_MAGIC:
        raise RuntimeError(f'Magic {magic}, not "{BTRFS_SEND_STREAM_MAGIC}"')


def check_version(infile, expected) -> None:
    (version,) = file_unpack("<I", infile)
    if version != expected:
        raise RuntimeError(
            f"Got version {version}, but expected version {expected}"
        )


class CommandKind(enum.Enum):
    # If we see one of these, it's an error: UNSPEC = 0

    SUBVOL = 1
    SNAPSHOT = 2

    MKFILE = 3
    MKDIR = 4
    MKNOD = 5
    MKFIFO = 6
    MKSOCK = 7
    SYMLINK = 8

    RENAME = 9
    LINK = 10
    UNLINK = 11
    RMDIR = 12

    SET_XATTR = 13
    REMOVE_XATTR = 14

    WRITE = 15
    CLONE = 16

    TRUNCATE = 17
    CHMOD = 18
    CHOWN = 19
    UTIMES = 20

    END = 21
    UPDATE_EXTENT = 22


class CommandHeader(NamedTuple):
    kind: CommandKind
    length: int  # excluding the header
    crc: int  # including the header, with this field set to 0

    @staticmethod
    def from_file(infile) -> "CommandHeader":
        length, kind, crc = file_unpack("<IHI", infile)
        return CommandHeader(kind=CommandKind(kind), length=length, crc=crc)


class AttributeKind(enum.Enum):
    # If we see one of these, it's an error: UNSPEC = 0

    UUID = 1
    CTRANSID = 2

    # NB: INO occurs in the send-stream, but `btrfs-progs` currently does
    # not use it in any way.
    INO = 3
    SIZE = 4
    MODE = 5
    UID = 6
    GID = 7
    RDEV = 8
    CTIME = 9
    MTIME = 10
    ATIME = 11
    # Unused: OTIME = 12

    XATTR_NAME = 13
    XATTR_DATA = 14

    PATH = 15
    PATH_TO = 16
    PATH_LINK = 17

    FILE_OFFSET = 18
    DATA = 19

    CLONE_UUID = 20
    CLONE_CTRANSID = 21
    CLONE_PATH = 22
    CLONE_OFFSET = 23
    CLONE_LEN = 24


class AttributeHeader(NamedTuple):
    kind: AttributeKind
    length: int  # excluding the header

    @staticmethod
    def from_file(infile) -> "AttributeHeader":
        kind, length = file_unpack("<HH", infile)
        return AttributeHeader(kind=AttributeKind(kind), length=length)


def conv_uuid(s: bytes) -> bytes:
    return str(uuid.UUID(bytes=s)).encode()  # All our other strings are bytes


def conv_uint64(s: bytes) -> int:
    (i,) = struct.unpack("<Q", s)
    return i


def conv_time(s: bytes) -> Tuple[int, int]:
    s, us = struct.unpack("<QI", s)
    # pyre wants an explicit check even though struct.unpack will raise
    assert isinstance(s, int) and isinstance(us, int), "struct.unpack() failed"
    return s, us


def read_attribute(infile):
    attr_header = AttributeHeader.from_file(infile)
    attr_data = infile.read(attr_header.length)
    if len(attr_data) != attr_header.length:
        raise RuntimeError(f"{attr_header} got {len(attr_data)} bytes")

    if attr_header.kind == AttributeKind.UUID:
        return attr_header.kind, conv_uuid(attr_data)
    elif attr_header.kind == AttributeKind.CTRANSID:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.INO:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.SIZE:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.MODE:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.UID:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.GID:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.RDEV:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.CTIME:
        return attr_header.kind, conv_time(attr_data)
    elif attr_header.kind == AttributeKind.MTIME:
        return attr_header.kind, conv_time(attr_data)
    elif attr_header.kind == AttributeKind.ATIME:
        return attr_header.kind, conv_time(attr_data)
    elif attr_header.kind == AttributeKind.XATTR_NAME:
        return attr_header.kind, attr_data
    elif attr_header.kind == AttributeKind.XATTR_DATA:
        return attr_header.kind, attr_data
    elif attr_header.kind == AttributeKind.PATH:
        return attr_header.kind, os.path.normpath(attr_data)
    elif attr_header.kind == AttributeKind.PATH_TO:
        return attr_header.kind, os.path.normpath(attr_data)
    elif attr_header.kind == AttributeKind.PATH_LINK:
        # NB This is NOT normalized since we don't want to normalize symlinks
        return attr_header.kind, attr_data
    elif attr_header.kind == AttributeKind.FILE_OFFSET:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.DATA:
        return attr_header.kind, attr_data
    elif attr_header.kind == AttributeKind.CLONE_UUID:
        return attr_header.kind, conv_uuid(attr_data)
    elif attr_header.kind == AttributeKind.CLONE_CTRANSID:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.CLONE_PATH:
        return attr_header.kind, os.path.normpath(attr_data)
    elif attr_header.kind == AttributeKind.CLONE_OFFSET:
        return attr_header.kind, conv_uint64(attr_data)
    elif attr_header.kind == AttributeKind.CLONE_LEN:
        return attr_header.kind, conv_uint64(attr_data)

    raise RuntimeError(f"Fix me: unhandled {attr_header}")  # pragma: no cover


def read_command(infile):
    cmd_header = CommandHeader.from_file(infile)

    s = infile.read(cmd_header.length)
    if len(s) != cmd_header.length:
        raise RuntimeError(f"{cmd_header} got {len(s)} bytes")
    # Future: pull in the `crc32c` module and check the CRC.

    attr_bytes = BytesIO(s)
    kind_to_attr = {}
    while attr_bytes.tell() != len(s):
        kind, attr = read_attribute(attr_bytes)
        if kind in kind_to_attr:
            raise RuntimeError(f"{kind} occurred twice in {cmd_header}")
        kind_to_attr[kind] = attr

    if cmd_header.kind == CommandKind.SUBVOL:
        return SendStreamItems.subvol(
            path=kind_to_attr[AttributeKind.PATH],
            uuid=kind_to_attr[AttributeKind.UUID],
            transid=kind_to_attr[AttributeKind.CTRANSID],
        )
    elif cmd_header.kind == CommandKind.SNAPSHOT:
        return SendStreamItems.snapshot(
            path=kind_to_attr[AttributeKind.PATH],
            uuid=kind_to_attr[AttributeKind.UUID],
            transid=kind_to_attr[AttributeKind.CTRANSID],
            parent_uuid=kind_to_attr[AttributeKind.CLONE_UUID],
            parent_transid=kind_to_attr[AttributeKind.CLONE_CTRANSID],
        )
    elif cmd_header.kind == CommandKind.MKFILE:
        return SendStreamItems.mkfile(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.MKDIR:
        return SendStreamItems.mkdir(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.MKNOD:
        return SendStreamItems.mknod(
            path=kind_to_attr[AttributeKind.PATH],
            mode=kind_to_attr[AttributeKind.MODE],
            dev=kind_to_attr[AttributeKind.RDEV],
        )
    elif cmd_header.kind == CommandKind.MKFIFO:
        return SendStreamItems.mkfifo(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.MKSOCK:
        return SendStreamItems.mksock(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.SYMLINK:
        return SendStreamItems.symlink(
            path=kind_to_attr[AttributeKind.PATH],
            # NB Unlike the other `dest` attributes, we don't normalize this.
            dest=os.path.normpath(kind_to_attr[AttributeKind.PATH_LINK]),
        )
    elif cmd_header.kind == CommandKind.RENAME:
        return SendStreamItems.rename(
            path=kind_to_attr[AttributeKind.PATH],
            dest=kind_to_attr[AttributeKind.PATH_TO],
        )
    elif cmd_header.kind == CommandKind.LINK:
        return SendStreamItems.link(
            path=kind_to_attr[AttributeKind.PATH],
            dest=os.path.normpath(kind_to_attr[AttributeKind.PATH_LINK]),
        )
    elif cmd_header.kind == CommandKind.UNLINK:
        return SendStreamItems.unlink(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.RMDIR:
        return SendStreamItems.rmdir(path=kind_to_attr[AttributeKind.PATH])
    elif cmd_header.kind == CommandKind.WRITE:
        return SendStreamItems.write(
            path=kind_to_attr[AttributeKind.PATH],
            offset=kind_to_attr[AttributeKind.FILE_OFFSET],
            data=kind_to_attr[AttributeKind.DATA],
        )
    elif cmd_header.kind == CommandKind.CLONE:
        return SendStreamItems.clone(
            path=kind_to_attr[AttributeKind.PATH],
            offset=kind_to_attr[AttributeKind.FILE_OFFSET],
            len=kind_to_attr[AttributeKind.CLONE_LEN],
            from_uuid=kind_to_attr[AttributeKind.CLONE_UUID],
            from_transid=kind_to_attr[AttributeKind.CLONE_CTRANSID],
            from_path=kind_to_attr[AttributeKind.CLONE_PATH],
            clone_offset=kind_to_attr[AttributeKind.CLONE_OFFSET],
        )
    elif cmd_header.kind == CommandKind.SET_XATTR:
        return SendStreamItems.set_xattr(
            path=kind_to_attr[AttributeKind.PATH],
            name=kind_to_attr[AttributeKind.XATTR_NAME],
            data=kind_to_attr[AttributeKind.XATTR_DATA],
        )
    elif cmd_header.kind == CommandKind.REMOVE_XATTR:
        return SendStreamItems.remove_xattr(
            path=kind_to_attr[AttributeKind.PATH],
            name=kind_to_attr[AttributeKind.XATTR_NAME],
        )
    elif cmd_header.kind == CommandKind.TRUNCATE:
        return SendStreamItems.truncate(
            path=kind_to_attr[AttributeKind.PATH],
            size=kind_to_attr[AttributeKind.SIZE],
        )
    elif cmd_header.kind == CommandKind.CHMOD:
        return SendStreamItems.chmod(
            path=kind_to_attr[AttributeKind.PATH],
            mode=kind_to_attr[AttributeKind.MODE],
        )
    elif cmd_header.kind == CommandKind.CHOWN:
        return SendStreamItems.chown(
            path=kind_to_attr[AttributeKind.PATH],
            uid=kind_to_attr[AttributeKind.UID],
            gid=kind_to_attr[AttributeKind.GID],
        )
    elif cmd_header.kind == CommandKind.UTIMES:
        return SendStreamItems.utimes(
            path=kind_to_attr[AttributeKind.PATH],
            ctime=kind_to_attr[AttributeKind.CTIME],
            mtime=kind_to_attr[AttributeKind.MTIME],
            atime=kind_to_attr[AttributeKind.ATIME],
        )
    elif cmd_header.kind == CommandKind.END:
        return None
    elif cmd_header.kind == CommandKind.UPDATE_EXTENT:
        return SendStreamItems.update_extent(
            path=kind_to_attr[AttributeKind.PATH],
            offset=kind_to_attr[AttributeKind.FILE_OFFSET],
            len=kind_to_attr[AttributeKind.SIZE],
        )

    raise AssertionError(f"Fix me: unhandled {cmd_header}")  # pragma: no cover


def parse_send_stream(infile) -> Iterator[SendStreamItem]:
    check_magic(infile)
    check_version(infile, expected=1)
    while True:
        cmd = read_command(infile)
        if cmd is None:
            return
        yield cmd
