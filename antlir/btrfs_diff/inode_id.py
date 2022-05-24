#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Our inodes' primary purpose is testing. However, writing tests against
arbtirarily selected integer inode IDs is unnecessarily hard.  For this
reason, InodeIDs are tightly integrated with a path mapping, which is used
to represent the Inode instead of the underlying integer ID, whenever
possible.
"""
import itertools
import os
from collections import defaultdict, deque
from typing import (
    Any,
    Iterator,
    Mapping,
    NamedTuple,
    Optional,
    Sequence,
    Set,
    Tuple,
)

from .freeze import freeze


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def tail(n: int, iterable):
    "Return an iterator over the last n items"
    return iter(deque(iterable, maxlen=n))  # tail(3, 'ABCDEFG') --> E F G


class InodeID(NamedTuple):
    """
    IMPORTANT: To support `Subvolume` snapshots, this must be correctly
    `deepcopy`able in a copy operation that directly includes its
    `.inner_id_map`.  I mean "directly" in the sense that we must also copy
    the ground-truth reference to our `InodeIDMap`, i.e.  via the field of
    `Subvolume`.  In contrast, `deepcopy`ing `InodeID`s without copying the
    whole map would result in decoupling between those objects, which is
    incorrect.
    """

    id: int
    # While this field creates some aliasing issues with `deepcopy` (see
    # the doblock), it is still worthwhile to have it:
    #  - It makes it trivial to aggregate cloned extents across subvolumes
    #    in `SubvolumeSet.freeze`, since IDs from different maps differ.
    #  - We check `inner_id_map` identity at runtime (below) to ensure at
    #    runtime that `InodeID`s are used only with their maps.
    #  - An identifiable repr is nice for ease of testing/debugging.
    inner_id_map: "_InnerInodeIDMap"

    def __repr__(self) -> str:
        paths = list(self.inner_id_map.gen_paths(self))
        desc = ""
        if self.inner_id_map.description:
            desc = f"{self.inner_id_map.description}@"
        if not paths:
            return f"{desc}ANON_INODE#{self.id}"
        return desc + ",".join(
            p.decode(errors="surrogateescape") for p in sorted(paths)
        )

    # `_InnerInodeIDMap` is a tuple, which gives it "plain old data"
    # semantics for hashing and equality.  We actually don't want that for
    # the purposes of comparing `InodeID`s, not least because
    #  - `_InnerInodeIDMap` is unhashable, but `InodeID`s are desired as
    #    dict keys.
    #  - The goal in comparing `inner_id_map` is **only** to verify object
    #    identity.  We don't care if two different objects have the same
    #    content.

    def __hash__(self) -> int:
        return hash((self.id, id(self.inner_id_map)))

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __eq__(self, other):
        return (
            type(self) is type(other)
            and self.id == other.id
            and self.inner_id_map is other.inner_id_map
        )


def _norm_split_path(p: bytes) -> Sequence[bytes]:
    # Check explicitly since the downstream errors are incomprehensible.
    if not isinstance(p, bytes):
        raise TypeError(f"Expected bytes, got {p}")
    p = os.path.normpath(p)
    if os.path.isabs(p):
        raise ValueError(f"Need relative path, got {p}")
    return [] if p == b"." else p.split(b"/")


# forward declaration so that is_root() type checks
_ROOT_REVERSE_ENTRY: "_ReversePathEntry"


class _ReversePathEntry(NamedTuple):
    """
    Reading `.name` and following `.parent_int_id` until it is None gives
    you a right-to-left reading of the path from the root.
    """

    name: bytes  # The path component
    # Pointer to the previous path component.
    #
    # This is logically a `_ReversePathEntry`, but rename operations can
    # cause the content of the parent `_ReversePathEntry` to change.  Since
    # the object is immutable, we cannot store a direct reference t our
    # parent, and must instead use IDs for indirection.  We use an integer
    # instead of `InodeID` to prevent a circular dependency.
    parent_int_id: Optional[int]

    def is_root(self) -> bool:
        return self == _ROOT_REVERSE_ENTRY


_ROOT_REVERSE_ENTRY = _ReversePathEntry(name=b"", parent_int_id=None)


# pyre-fixme[2]: Parameter annotation cannot be `Any`.
class _InnerInodeIDMap(NamedTuple):
    "Explained where `InodeIDMap.inner` is declared."
    # pyre-fixme[4]: Attribute annotation cannot be `Any`.
    description: Any  # repr()able, to be used for repr()ing InodeIDs
    # The key is not an `InodeID` to avoid a circular dependency.  The
    # values correspond to different hardlinks to the same file inode.
    # Directories will always have a single element in the set.
    id_to_reverse_entries: Mapping[int, Set[_ReversePathEntry]]

    def _assert_mine(self, inode_id: InodeID) -> InodeID:
        if inode_id.inner_id_map is not self:
            # Avoid InodeID.__repr__ since that would recurse infinitely.
            raise RuntimeError(f"Wrong map for InodeID #{inode_id.id}")
        return inode_id

    def _rev_entry_to_path(
        self, rev_entry: _ReversePathEntry
    ) -> Iterator[bytes]:
        parent_id = rev_entry.parent_int_id
        assert parent_id is not None, "Never called with _ROOT_REVERSE_ENTRY"
        (  # Directories don't have hardlink, so they have just 1 reverse entry
            parent,
        ) = self.id_to_reverse_entries[parent_id]
        if not parent.is_root():
            yield from self._rev_entry_to_path(parent)
        yield rev_entry.name

    def gen_paths(self, inode_id: InodeID) -> Iterator[bytes]:
        for rev_entry in self.id_to_reverse_entries.get(
            self._assert_mine(inode_id).id, ()  # we tolerate anonymous inodes
        ):
            if rev_entry.is_root():
                yield b"."
            else:
                yield b"/".join(self._rev_entry_to_path(rev_entry))


class _PathEntry(NamedTuple):
    id: InodeID
    # `None` -> the entry is a file, a mapping -> it's a directory.
    name_to_child: Optional[Mapping[bytes, "_PathEntry"]]


class InodeIDMap(NamedTuple):
    """
    Path -> Inode mapping, represents the directory structure of a filesystem.

    All paths should be relative. Use b'.' to refer to the root of this map.

    Unlike a real filesystem, this does not resolve symlinks.

    IMPORTANT: Keep this object `deepcopy`able for the purpose of
    snapshotting subvolumes -- it currently has a test to check this, but
    the test may not catch every kind of copy-related problem.  In
    particular, because `description` has type `Any`, it can bring
    `deepcopy` issues -- see the notes on the `deepcopy`ability
    of `SubvolumeDescription` in `volume.py` to understand the risks.
    """

    inode_id_counter: Iterator[int]
    # `_PathEntry.id`s contain references to `self.inner`.
    root: _PathEntry
    # This structure is separated from `self` so that `InodeID`s do NOT have
    # a circular dependency on `InodeIDMap`.  This dependency-factoring is
    # necessary so that our `freeze()` can make a recursively-immutable
    # variant of `InodeIDMap`.
    inner: _InnerInodeIDMap

    @classmethod
    # pyre-fixme[2]: Parameter annotation cannot be `Any`.
    def new(cls, *, description: Any = "") -> "InodeIDMap":
        inner = _InnerInodeIDMap(
            description=description, id_to_reverse_entries=defaultdict(set)
        )
        counter = itertools.count()
        self = cls(
            inode_id_counter=counter,
            root=_PathEntry(
                id=InodeID(id=next(counter), inner_id_map=inner),
                name_to_child={},
            ),
            inner=inner,
        )
        self.inner.id_to_reverse_entries[self.root.id.id].add(
            _ROOT_REVERSE_ENTRY
        )
        return self

    # pyre-fixme[3]: Return type must be annotated.
    def freeze(self, *, _memo):
        "Returns a recursively immutable copy of `self`."
        return self._make(
            freeze(i, _memo=_memo)  # can't add IDs once frozen
            for i in self._replace(inode_id_counter=None)
        )

    def next(self) -> InodeID:
        return InodeID(id=next(self.inode_id_counter), inner_id_map=self.inner)

    def _gen_entries(
        self, parts: Sequence[bytes]
    ) -> Iterator[Optional[_PathEntry]]:
        entry = self.root
        yield entry
        for name in parts:
            maybe_map = entry.name_to_child
            if maybe_map is None:
                raise RuntimeError(f"{name}'s parent in {parts} is a file")
            entry = maybe_map.get(name)
            yield entry
            if entry is None:
                # The path is missing some ancestors -- our callers handle
                # this differently.  A last value of `None` is a sentinel.
                break

    def _get_parts_parent_and_entry(
        self, path: bytes
    ) -> Tuple[Sequence[bytes], _PathEntry, _PathEntry]:
        "Contract: never call this on the root, aka empty `parts`"
        parts = _norm_split_path(path)
        if not parts:
            raise RuntimeError("Cannot remove the root path")
        parent, entry = tail(2, self._gen_entries(parts))
        if entry is None:
            raise RuntimeError(f"Cannot remove non-existent {path}")
        return parts, parent, entry

    # We must differentiate between files and directories because hardlinks
    # to directories would cause a combinatorial explosion of possible paths
    # to a file, which would unnecessarily complicate our implementation.

    def add_file(self, ino_id: InodeID, path: bytes) -> InodeID:
        self._add_path(_PathEntry(id=ino_id, name_to_child=None), path)
        return ino_id

    def add_dir(self, ino_id: InodeID, path: bytes) -> InodeID:
        self._add_path(_PathEntry(id=ino_id, name_to_child={}), path)
        return ino_id

    def _add_path(self, entry: _PathEntry, path: bytes) -> None:
        self.inner._assert_mine(entry.id)

        # Block an ID from being added as both a file and a directory, ban
        # directory hardlinks.
        for prev_path in self.inner.gen_paths(entry.id):
            prev_entry = self._get_entry(prev_path)
            if (entry.name_to_child, prev_entry.name_to_child) != (None, None):
                raise RuntimeError(
                    f"Tried to add non-file hardlink for {entry.id}"
                )
            break  # It's enough to check 1 entry

        parts = _norm_split_path(path)
        (parent,) = tail(1, self._gen_entries(parts[:-1]))
        if parent is None:
            raise RuntimeError(f"Missing ancestor for {path}")
        if parent.name_to_child is None:
            raise RuntimeError(f"The parent of {path} is a file")

        old = parent.name_to_child.get(parts[-1])
        if old is not None:
            raise RuntimeError(
                f"Adding #{entry.id.id} to {path} which has #{old.id.id}"
            )

        reverse_parent = self.inner.id_to_reverse_entries.get(parent.id.id)
        assert isinstance(reverse_parent, set) and len(reverse_parent) == 1

        parent.name_to_child[parts[-1]] = entry
        self.inner.id_to_reverse_entries[entry.id.id].add(
            _ReversePathEntry(name=parts[-1], parent_int_id=parent.id.id)
        )

    def remove_path(self, path: bytes) -> InodeID:
        _parts, parent, entry = self._get_parts_parent_and_entry(path)
        if entry.name_to_child:
            raise RuntimeError(f"Cannot remove {path} since it has children")
        return self._remove_path_unsafe(path).id

    def _reverse_entry_matches_path_parts(
        self, reverse_entry: _ReversePathEntry, parts: Sequence[bytes]
    ) -> bool:
        for part in reversed(parts):
            maybe_id = reverse_entry.parent_int_id
            if reverse_entry.is_root() or maybe_id is None:
                return False  # `parts` is longer than the path to the root
            if part != reverse_entry.name:
                return False  # Different paths
            entries = self.inner.id_to_reverse_entries.get(maybe_id)
            assert isinstance(entries, set) and len(entries) == 1
            (reverse_entry,) = entries
        # Since `parts` never has a component corresponding to the root
        # inode, if we got this far, it must be that all of `parts` had a
        # name match.
        #
        # If we're not at the root, then `parts` accidentally happened to be
        # a suffix of this `_ReversePathEntry`, but it's not a match.
        return reverse_entry.is_root()

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def _matching_reverse_path_entry(self, reverse_entries, parts):
        """
        File hardlinks means an inode will have many `reverse_path_entries`,
        so we'll check each one to find the one that matches `parts`.
        Invariant: `reverse_entries` corresponds to an existing `_PathEntry`
        whose path is in `parts`.
        """
        if not reverse_entries:  # pragma: no cover
            raise AssertionError(f"{parts} entry has no _ReversePathEntry")
        # See if any of the `ReverseParentEntries` match `parts`.
        for reverse_entry in reverse_entries:
            if self._reverse_entry_matches_path_parts(reverse_entry, parts):
                return reverse_entry
        raise AssertionError(  # pragma: no cover
            f"No _ReversePathEntry matched {parts}"
        )

    def _remove_path_unsafe(self, path: bytes) -> _PathEntry:
        "Does not check if path has children, used by `rename_path`."
        parts, parent, entry = self._get_parts_parent_and_entry(path)

        maybe_map = parent.name_to_child
        assert maybe_map is not None, "parent must have name_to_child map"

        del maybe_map[parts[-1]]

        entries = self.inner.id_to_reverse_entries[entry.id.id]
        entries.remove(self._matching_reverse_path_entry(entries, parts))
        if not entries:
            del self.inner.id_to_reverse_entries[entry.id.id]

        return entry

    def rename_path(self, src: bytes, dest: bytes) -> None:
        """
        It may be tempting to `add_*(remove_*(src), dest)`. However,
        that idiom:
         - would break on nonempty directories,
         - is not exception-safe, since the add can fail after the remove
           succeeded.
        """
        entry = self._remove_path_unsafe(src)
        try:
            self._add_path(entry, dest)
        except Exception:
            self._add_path(entry, src)
            raise

    def _get_entry(self, path: bytes) -> _PathEntry:
        (entry,) = tail(1, self._gen_entries(_norm_split_path(path)))
        return entry

    def get_id(self, path: bytes) -> Optional[InodeID]:
        """
        Returns None if the path does not exist, raises if the path
        contains a file as a non-final component.
        """
        entry = self._get_entry(path)
        return None if entry is None else entry.id

    def get_paths(self, inode_id: InodeID) -> Set[bytes]:
        return set(self.inner.gen_paths(inode_id))

    def get_children(self, inode_id: InodeID) -> Optional[Set[bytes]]:
        """
        Returns None if the is a file, raises if the path contains a file as
        a non-final component.
        """
        paths = list(self.inner.gen_paths(inode_id))
        if len(paths) > 1:  # Directories have 1 path, 0 paths is an error
            return None  # A file
        (path,) = paths
        entry = self._get_entry(path)  # Not None since we started from InodeID
        maybe_map = entry.name_to_child
        if maybe_map is None:
            return None
        else:
            return {
                os.path.normpath(os.path.join(path, name)) for name in maybe_map
            }
