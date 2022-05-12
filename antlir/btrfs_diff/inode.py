#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Inode, and the structures it contains, represent the final, constructed
state of the filesystem. They are immutable, designed to be easy to
construct, and easy to compare in tests.

## Note on `__repr__`

These are used for tests, so they must be compact & reasonably lossless.
Avoid whitespace when possible, since IncompleteInode uses space separators.
"""
import stat
from datetime import datetime
from typing import MutableMapping, NamedTuple, Optional, Sequence, Set, Tuple

from .extent import Extent
from .inode_id import InodeID


class InodeOwner(NamedTuple):
    uid: int
    gid: int

    def __repr__(self):
        return f"{self.uid}:{self.gid}"


MSEC_TO_NSEC = 10**6
SEC_TO_NSEC = 1000 * MSEC_TO_NSEC
MIN_TO_SEC = 60
HOUR_TO_SEC = 60 * MIN_TO_SEC
DAY_TO_SEC = 24 * HOUR_TO_SEC


def _time_delta(a: Tuple[int, int], b: Tuple[int, int]) -> Tuple[int, int]:
    "returns (sec, nsec) -- sec may be negative, nsec is always positive"
    sec_diff = a[0] - b[0]
    nsec_diff = a[1] - b[1]
    nsec = nsec_diff % SEC_TO_NSEC
    nsec_excess = nsec_diff - nsec
    assert nsec_excess % SEC_TO_NSEC == 0, f"{a} - {b}"
    return (sec_diff + nsec_excess // SEC_TO_NSEC, nsec)


def _add_nsec_to_repr(prev: str, nsec: int) -> str:
    """
    Truncate to milliseconds for compactness, our tests should not care.
    We do NOT round up (too much code), so 999999000 renders as 999.
    """
    return f"{prev}.{nsec // MSEC_TO_NSEC:03}".rstrip("0").rstrip(".")


def _repr_time_delta(sec: int, nsec: int) -> str:
    "sec may be negative, nsec is always positive"
    if sec < 0:
        sign = "-"
        sec = -sec
        if nsec > 0:
            sec -= 1
            nsec = SEC_TO_NSEC - nsec
    else:
        sign = "+"
    return _add_nsec_to_repr(f"{sign}{sec}", nsec)


def _repr_time(sec: int, nsec: int) -> str:
    sec_str = datetime.utcfromtimestamp(sec).strftime("%y/%m/%d.%H:%M:%S")
    return _add_nsec_to_repr(sec_str, nsec)


class InodeUtimes(NamedTuple):
    ctime: Tuple[int, int]  # sec, nsec
    mtime: Tuple[int, int]
    atime: Tuple[int, int]

    def __repr__(self):
        c_to_m = _repr_time_delta(*_time_delta(self.mtime, self.ctime))
        m_to_a = _repr_time_delta(*_time_delta(self.atime, self.mtime))
        return f"{_repr_time(*self.ctime)}{c_to_m}{m_to_a}"


S_IFMT_TO_FILE_TYPE_NAME = {
    stat.S_IFBLK: "Block",
    stat.S_IFCHR: "Char",
    stat.S_IFDIR: "Dir",
    stat.S_IFIFO: "FIFO",
    stat.S_IFLNK: "Symlink",
    stat.S_IFREG: "File",
    stat.S_IFSOCK: "Sock",
}

EXTENT_KIND_TO_ABBREV = {Extent.Kind.HOLE: "h", Extent.Kind.DATA: "d"}


def _repr_decode(b: bytes) -> str:
    return b.decode(errors="surrogateescape")


# Future: `frozentype` should let us mirror the `Incomplete*` hierarchy,
# instead of making this enum + union type hack.
class Inode(NamedTuple):
    # All inode types have these first 5 fields

    file_type: int  # Upper bits of `st_mode` matching `S_IFMT`
    # The next 3 fields are not actually optional, but they may be `None` if
    # the `Inode` was made from a partly-populated `IncompleteInode`.
    mode: Optional[int]  # Bottom 12 bits of `st_mode`
    owner: Optional[InodeOwner]
    utimes: Optional[InodeUtimes]
    xattrs: MutableMapping[bytes, bytes]

    # The subsequent fields are specific to particular file_types.  `_new`
    # will assert that they are not None iff they are relevant.

    # FILE
    #
    # The inode's data fork is a concatenation of Chunks, computed from a
    # set of `Extent`s by `extents_to_chunks_with_clones`.
    chunks: Optional[Sequence["Chunk"]] = None

    # DEVICE -- block vs character is encoded in `file_type`
    dev: Optional[int] = None

    # SYMLINK
    dest: Optional[bytes] = None

    def assert_valid_and_complete(self):
        if None in (self.file_type, self.owner, self.utimes):
            raise RuntimeError(f"{self} must have file_type, owner & utimes")
        if stat.S_ISLNK(self.file_type) ^ (self.mode is None):
            raise RuntimeError(f"only symlinks must omit mode, got {self}")
        if self.file_type & ~(stat.S_IFMT(self.file_type)):
            raise RuntimeError(f"bad .file_type bits in {self}")
        if self.mode is not None and (self.mode & stat.S_IFMT(self.mode)):
            raise RuntimeError(f"bad .mode bits in {self}")
        if (self.chunks is not None) ^ stat.S_ISREG(self.file_type):
            raise RuntimeError(f"{self} must have .chunks iff it is a file")
        is_dev = stat.S_ISBLK(self.file_type) or stat.S_ISCHR(self.file_type)
        if (self.dev is not None) ^ is_dev:
            raise RuntimeError(f"{self} must have .dev iff it is a device")
        if (self.dest is not None) ^ stat.S_ISLNK(self.file_type):
            raise RuntimeError(f"{self} must have .dest iff it is a symlink")

    def _repr_fields(self):
        yield S_IFMT_TO_FILE_TYPE_NAME.get(self.file_type, str(self.file_type))
        if self.mode is not None:
            yield f"m{self.mode:o}"
        if self.owner is not None:
            yield f"o{self.owner}"
        if self.utimes is not None:
            yield f"t{self.utimes}"
        if self.xattrs:
            yield "x" + ",".join(
                f"{repr(_repr_decode(k))}={repr(_repr_decode(v))}"
                for k, v in self.xattrs.items()
            )
        # The next 3 fields are unmarked because they belong to different
        # file-types, and would normally come last.  A pathological `Inode`
        # could emit ambiguous output because of this, but I'm not worried.
        if self.chunks:  # This won't distinguish between `None` and `()`
            yield "".join(
                f"{EXTENT_KIND_TO_ABBREV[c.kind]}{c.length}"
                + (
                    (
                        "("
                        + "/".join(sorted(repr(cc) for cc in c.chunk_clones))
                        + ")"
                    )
                    if c.chunk_clones
                    else ""
                )
                for c in self.chunks
            )
        if self.dev is not None:
            yield f"{hex(self.dev)[2:]}"
        if self.dest is not None:
            yield f"{_repr_decode(self.dest)}"

    def __repr__(self):
        return "(" + " ".join(self._repr_fields()) + ")"


class Clone(NamedTuple):
    "A reference to a byte interval in an Inode."
    # We could not use Inode objects here, since it's completely reasonable
    # for inode A to contain a clone from B, while B contains a clone from
    # A.  Objects with direct circular dependencies cannot be constructed,
    # so we need the indirection.
    inode_id: InodeID
    offset: int  # The byte offset into the data fork of the `inode_id` Inode.
    length: int

    def __repr__(self):
        return f"{self.inode_id}:{self.offset}+{self.length}"


class ChunkClone(NamedTuple):
    # Clones are only parts of a chunk. The offset of the clone within this
    # chunk is outside of `Clone` to simplify chunk merging.
    offset: int  # Offset into the `Chunk`
    clone: Clone  # What byte range in which Inode does this clone?

    def __repr__(self):
        return f"{repr(self.clone)}@{self.offset}"


class Chunk(NamedTuple):
    kind: Extent.Kind
    length: int
    chunk_clones: Set[ChunkClone]

    def __repr__(self):
        return (
            f"({self.kind.name}/{self.length}"
            + (
                (": " + ", ".join(repr(c) for c in self.chunk_clones))
                if self.chunk_clones
                else ""
            )
            + ")"
        )
