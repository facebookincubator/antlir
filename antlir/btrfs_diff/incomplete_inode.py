#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
To construct our filesystem, it is convenient to have mutable classes that
track the state-in-progress.  The `IncompleteInode` hierarchy stores that
state, and knows how to apply parsed `SendStreamItems` to mutate the state.

Once the filesystem is done, we will "freeze" it into immutable, hashable,
easily comparable `Inode` objects, making it a "breeze" to validate it.

IMPORTANT: Keep these objects correctly `deepcopy`able. That is the case at
the time of writing because:
 - `Extent` is recursively immutable and customizes copy operations to
   return the original object -- this lets us correctly track clones.
 - All other attributes store plain-old-data, or POD immutable classes that
   do not care about object identity.
 - We omit InodeID -- i.e. these objects are **just** the inode's data.
   This is important because InodeID contains an InodeIDMap reference, which
   means that correctly copying IncompleteInodes that contain InodeIDs would
   require one to copy **only** at a high enough scope of the hierarchy that
   both the InodeIDMap and all the relevant IncompleteInodes are included.
   This extra risk doesn't seem worth the debuggability reward of having
   IncompleteInodes know their identity.

Future: with `deepfrozen` done, it would be simplest to merge
`IncompleteInode` with `Inode`, and just have `apply_item` return a
partly-modified copy, in the style of `NamedTuple._replace`.
"""
import itertools
import stat
from abc import ABC
from typing import Dict, Optional, Sequence, Type

from antlir.btrfs_diff.extent import Extent
from antlir.btrfs_diff.freeze import freeze
from antlir.btrfs_diff.inode import Chunk, Inode, InodeOwner, InodeUtimes
from antlir.btrfs_diff.parse_dump import SendStreamItem, SendStreamItems


class IncompleteInode(ABC):
    """
    Base class for all inode types. Inheritance is appropriate because
    different inode types have different data, different construction logic,
    and freezing logic.
    """

    file_type: int  # Upper bits of `st_mode` matching `S_IFMT`
    mode: Optional[int]  # Bottom 12 bits of `st_mode`
    owner: Optional[InodeOwner]
    utimes: Optional[InodeUtimes]
    xattrs: Dict[bytes, bytes]
    # these are pure virtual for children to implement/assign
    INITIAL_ITEM: Type
    FILE_TYPE: int
    # If any of these are None, the filesystem was created badly.
    # Exception: symlinks don't have permissions.

    def __init__(self, *, item: SendStreamItem) -> None:
        assert isinstance(item, self.INITIAL_ITEM)
        self.file_type = self.FILE_TYPE
        self.mode = None
        self.owner = None
        self.utimes = None
        self.xattrs = {}

    def freeze(self, *, _memo, chunks: Sequence[Chunk]) -> Inode:
        "Returns a recursively immutable `Inode` based on `self`."
        # NB: If any freezing bugs turn up in this implementation, consider
        # wrapping a single `freeze` around the `freeze_kwargs` call to
        # ensure that everything gets processed.
        ino = Inode(**self._freeze_kwargs(_memo=_memo, chunks=chunks))
        assert (ino.chunks is not None) ^ (chunks is None)
        return ino

    def _freeze_kwargs(self, *, _memo, chunks: Sequence[Chunk]):
        return {
            "file_type": self.file_type,
            "mode": self.mode,
            # No need to freeze owner/utimes, they're recursively immutable.
            "owner": self.owner,
            "utimes": self.utimes,
            "xattrs": freeze(self.xattrs, _memo=_memo),
        }

    def apply_item(self, item: SendStreamItem) -> None:
        assert not isinstance(item, SendStreamItems.clone), "Do .apply_clone()"
        if isinstance(item, SendStreamItems.remove_xattr):
            del self.xattrs[item.name]
        elif isinstance(item, SendStreamItems.set_xattr):
            self.xattrs[item.name] = item.data
        elif isinstance(item, SendStreamItems.chmod):
            if stat.S_IFMT(item.mode) != 0:
                raise RuntimeError(
                    f"{item} cannot change file type bits of {self}"
                )
            self.mode = item.mode
        elif isinstance(item, SendStreamItems.chown):
            self.owner = InodeOwner(uid=item.uid, gid=item.gid)
        elif isinstance(item, SendStreamItems.utimes):
            self.utimes = InodeUtimes(
                ctime=item.ctime, mtime=item.mtime, atime=item.atime
            )
        else:
            raise RuntimeError(f"{self} cannot apply {item}")

    def apply_clone(
        self, item: SendStreamItems.clone, from_ino: "IncompleteInode"
    ) -> None:
        raise RuntimeError(f"{self} cannot clone via {item} from {from_ino}")

    def __repr__(self):
        return repr(freeze(self, chunks=None))


class IncompleteDir(IncompleteInode):
    FILE_TYPE = stat.S_IFDIR
    INITIAL_ITEM = SendStreamItems.mkdir


class IncompleteFile(IncompleteInode):
    extent: Extent

    FILE_TYPE = stat.S_IFREG
    INITIAL_ITEM = SendStreamItems.mkfile

    def __init__(self, *, item: SendStreamItem) -> None:
        super().__init__(item=item)
        self.extent = Extent.empty()

    def _freeze_kwargs(self, *, _memo, chunks: Sequence[Chunk]):
        assert (chunks is None) ^ (self.extent is not None)
        # Future: we could make some assertions to check that the chunks
        # correspond to the extent.
        return {
            "chunks": freeze(chunks, _memo=_memo),
            **super()._freeze_kwargs(_memo=_memo, chunks=chunks),
        }

    def apply_item(self, item: SendStreamItem) -> None:
        if isinstance(item, SendStreamItems.truncate):
            self.extent = self.extent.truncate(length=item.size)
        elif isinstance(item, SendStreamItems.write):
            self.extent = self.extent.write(
                offset=item.offset, length=len(item.data)
            )
        elif isinstance(item, SendStreamItems.update_extent):
            self.extent = self.extent.write(offset=item.offset, length=item.len)
        else:
            super().apply_item(item=item)

    def apply_clone(
        self, item: SendStreamItems.clone, from_ino: IncompleteInode
    ) -> None:
        assert isinstance(item, SendStreamItems.clone)
        if not isinstance(from_ino, IncompleteFile):
            raise RuntimeError(f"Cannot {item} from {from_ino}")
        # The validation isn't required in the sense that `Extent.clone` is
        # meant to handle any input appropriately, but it's probably a
        # symptom of incorrect usage, so let's report a more useful error.
        if not (
            0 <= item.clone_offset < from_ino.extent.length
            and 0 < (item.clone_offset + item.len) <= from_ino.extent.length
        ):
            raise RuntimeError(f"Bad offset/len {item} to clone {from_ino}")
        self.extent = self.extent.clone(
            to_offset=item.offset,
            from_extent=from_ino.extent,
            from_offset=item.clone_offset,
            length=item.len,
        )

    def __repr__(self):
        return repr(
            freeze(
                self,
                chunks=tuple(
                    Chunk(
                        kind=kind,
                        length=sum(length for _, length in chunks),
                        chunk_clones=(),
                    )
                    for kind, chunks in itertools.groupby(
                        (
                            (extent.content, length)
                            for _, length, extent in self.extent.gen_trimmed_leaves()  # noqa: E501
                        ),
                        lambda c: c[0],
                    )
                ),
            )
        )


class IncompleteSocket(IncompleteInode):
    FILE_TYPE = stat.S_IFSOCK
    INITIAL_ITEM = SendStreamItems.mksock


class IncompleteFifo(IncompleteInode):
    FILE_TYPE = stat.S_IFIFO
    INITIAL_ITEM = SendStreamItems.mkfifo


class IncompleteDevice(IncompleteInode):
    dev: int

    INITIAL_ITEM = SendStreamItems.mknod

    def __init__(self, *, item: SendStreamItem) -> None:
        if not isinstance(item, self.INITIAL_ITEM):
            raise RuntimeError(
                f"unexpected {type(item)}, expected {self.INITIAL_ITEM}"
            )
        self.FILE_TYPE = stat.S_IFMT(item.mode)
        if self.FILE_TYPE not in (stat.S_IFBLK, stat.S_IFCHR):
            raise RuntimeError(f"unexpected device mode in {item}")
        super().__init__(item=item)
        # NB: At present, `btrfs send` redundantly sends a `chmod` after
        # device creation, but we've already saved the file type.
        self.mode = item.mode & ~self.FILE_TYPE
        self.dev = item.dev

    def _freeze_kwargs(self, *, _memo, chunks: Sequence[Chunk]):
        return {
            "dev": self.dev,
            **super()._freeze_kwargs(_memo=_memo, chunks=chunks),
        }


class IncompleteSymlink(IncompleteInode):
    dest: bytes

    FILE_TYPE = stat.S_IFLNK
    INITIAL_ITEM = SendStreamItems.symlink

    def __init__(self, *, item: SendStreamItem) -> None:
        if not isinstance(item, self.INITIAL_ITEM):
            raise RuntimeError(
                f"unexpected {type(item)}, expected {self.INITIAL_ITEM}"
            )
        super().__init__(item=item)
        self.dest = item.dest

    def _freeze_kwargs(self, *, _memo, chunks: Sequence[Chunk]):
        return {
            "dest": self.dest,
            **super()._freeze_kwargs(_memo=_memo, chunks=chunks),
        }

    def apply_item(self, item: SendStreamItem) -> None:
        if isinstance(item, SendStreamItems.chmod):
            raise RuntimeError(f"{item} cannot chmod symlink {self}")
        else:
            super().apply_item(item=item)
