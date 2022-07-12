#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Much of the data in our mock VFS layer lies at the level of inodes
(see `incomplete_inode.py`, `inode.py`, etc).  `Subvolume` in the
next level up -- it maps paths to inodes.

Just like `IncompleteInode`, it knows how to apply `SendStreamItems` to
mutate its state.

## Known issues

- For now, we model subvolumes as having completely independent path
  structures.  They are not "mounted" into any kind of common directory
  tree, so our code does not currently need to check if a path inside a
  subvolume actually belongs to an inner subvolume.  In particular, this
  means we don't need to check for cross-device hardlinks or renames.

- Right now, we assume that all paths are fully resolved (symlink path
  components do not work)This is true for btrfs send-streams.  However, a
  truly general VFS mock would resolve symlinks in path components as
  specified by the standard.

- Maximum path lengths are not checked.
"""
import os
from types import MappingProxyType
from typing import (
    Any,
    Coroutine,
    Mapping,
    NamedTuple,
    Optional,
    Sequence,
    Tuple,
    Union,
    ValuesView,
)

from antlir.btrfs_diff.coroutine_utils import while_not_exited
from antlir.btrfs_diff.extents_to_chunks import extents_to_chunks_with_clones
from antlir.btrfs_diff.freeze import freeze
from antlir.btrfs_diff.incomplete_inode import (
    IncompleteDevice,
    IncompleteDir,
    IncompleteFifo,
    IncompleteFile,
    IncompleteInode,
    IncompleteSocket,
    IncompleteSymlink,
)
from antlir.btrfs_diff.inode import Chunk, Inode
from antlir.btrfs_diff.inode_id import InodeID, InodeIDMap
from antlir.btrfs_diff.rendered_tree import RenderedTree, TraversalIDMaker
from antlir.btrfs_diff.send_stream import SendStreamItem, SendStreamItems


_DUMP_ITEM_TO_INCOMPLETE_INODE = {
    SendStreamItems.mkdir: IncompleteDir,
    SendStreamItems.mkfile: IncompleteFile,
    SendStreamItems.mksock: IncompleteSocket,
    SendStreamItems.mkfifo: IncompleteFifo,
    SendStreamItems.mknod: IncompleteDevice,
    SendStreamItems.symlink: IncompleteSymlink,
}


# Future: `deepfrozen` would let us lose the `new` methods on NamedTuples,
# and avoid `deepcopy`.
class Subvolume(NamedTuple):
    """
    Models a btrfs subvolume, knows how to apply SendStreamItem mutations
    to itself.

    IMPORTANT: Keep this object correctly `deepcopy`able, we need that
    for snapshotting. Notes:

      - `InodeIDMap` opaquely holds a `description`, which in practice
        is a `SubvolumeDescription` that is **NOT** safely `deepcopy`able
        unless the whole `Volume` is being copied in one call.  For
        single-volume snapshots, `SubvolumeSetMutator` has an icky
        workaround :)

      - The tests for `InodeIDMap` try to ensure that it is safely
        `deepcopy`able.  Changes to its members should be validated there.

      - Any references to `id_map` from inside `id_to_node` are handled
        correctly, since we copy the entire `Subvolume` object in a single
        operation and `deepcopy` understands object aliasing.

      - `IncompleteInode` descendants are correctly deepcopy-able despite
        the fact that `Extent` relies on object identity for clone-tracking.
        This is explained in the submodule docblock.
    """

    # Inodes & inode maps are per-subvolume because btrfs treats subvolumes
    # as independent entities -- we cannot `rename` or hard-link data across
    # subvolumes, both fail with `EXDEV (Invalid cross-device link)`.
    # (Aside: according to Chris Mason, this is required to enable correct
    # space accounting on a per-subvolume basis.) The only caveat to this is
    # that a cross-subvolume `rename` is permitted to change the location
    # where a subvolume is mounted within a volume, but this does not
    # require us to share inodes across subvolumes.
    id_map: InodeIDMap
    id_to_inode: Mapping[Optional[InodeID], Union[IncompleteInode, Inode]]

    @classmethod
    def new(cls, *, id_map, **kwargs) -> "Subvolume":
        kwargs.setdefault("id_to_inode", {})
        kwargs["id_to_inode"][id_map.get_id(b".")] = IncompleteDir(
            item=SendStreamItems.mkdir(path=b".")
        )
        return cls(id_map=id_map, **kwargs)

    def inode_at_path(
        self, path: bytes
    ) -> Optional[Union[IncompleteInode, Inode]]:
        id = self.id_map.get_id(path)
        # Using `[]` instead of `.get()` to assert that `id_to_inode`
        # remains a superset of `id_map`.  The converse is harder to check.
        return None if id is None else self.id_to_inode[id]

    def _require_inode_at_path(
        self, item: SendStreamItem, path: bytes
    ) -> Union[IncompleteInode, Inode]:
        ino = self.inode_at_path(path)
        if ino is None:
            raise RuntimeError(f"Cannot apply {item}, {path} does not exist")
        return ino

    def _delete(self, path: bytes) -> None:
        ino_id = self.id_map.remove_path(path)
        if not self.id_map.get_paths(ino_id):
            del self.id_to_inode[ino_id]

    def apply_item(self, item: SendStreamItem) -> None:
        for item_type, inode_class in _DUMP_ITEM_TO_INCOMPLETE_INODE.items():
            if isinstance(item, item_type):
                ino_id = self.id_map.next()
                if isinstance(item, SendStreamItems.mkdir):
                    self.id_map.add_dir(ino_id, item.path)
                else:
                    self.id_map.add_file(ino_id, item.path)
                assert ino_id not in self.id_to_inode
                # pyre-fixme[16]: This is supposed to be frozen!!!
                self.id_to_inode[ino_id] = inode_class(item=item)
                return  # Done applying item

        if isinstance(item, SendStreamItems.rename):
            if item.dest.startswith(item.path + b"/"):
                raise RuntimeError(f"{item} makes path its own subdirectory")

            old_id = self.id_map.get_id(item.path)
            if old_id is None:
                raise RuntimeError(f"source of {item} does not exist")
            new_id = self.id_map.get_id(item.dest)

            # Per `rename (2)`, renaming same-inode links has NO effect o_O
            if old_id == new_id:
                return

            # No destination path? Easy.
            if new_id is None:
                self.id_map.rename_path(item.path, item.dest)
                return

            # Overwrite an existing path.
            if isinstance(self.id_to_inode[old_id], IncompleteDir):
                new_ino = self.id_to_inode[new_id]
                # _delete() below will ensure that the destination is empty
                if not isinstance(new_ino, IncompleteDir):
                    raise RuntimeError(
                        f"{item} cannot overwrite {new_ino}, since a "
                        "directory may only overwrite an empty directory"
                    )
            elif isinstance(self.id_to_inode[new_id], IncompleteDir):
                raise RuntimeError(
                    f"{item} cannot overwrite a directory with a non-directory"
                )
            self._delete(item.dest)
            self.id_map.rename_path(item.path, item.dest)
            # NB: Per `rename (2)`, if either the new or the old inode is a
            # symbolic link, they get treated just as regular files.
        elif isinstance(item, SendStreamItems.unlink):
            if isinstance(self.inode_at_path(item.path), IncompleteDir):
                raise RuntimeError(f"Cannot {item} a directory")
            self._delete(item.path)
        elif isinstance(item, SendStreamItems.rmdir):
            if not isinstance(self.inode_at_path(item.path), IncompleteDir):
                raise RuntimeError(f"Can only {item} a directory")
            self._delete(item.path)
        elif isinstance(item, SendStreamItems.link):
            if self.id_map.get_id(item.path) is not None:
                raise RuntimeError(f"Destination of {item} already exists")
            old_id = self.id_map.get_id(item.dest)
            if old_id is None:
                raise RuntimeError(f"{item} source does not exist")
            if isinstance(self.id_to_inode[old_id], IncompleteDir):
                raise RuntimeError(f"Cannot {item} a directory")
            self.id_map.add_file(old_id, item.path)
        else:  # Any other operation must be handled at inode scope.
            ino = self.inode_at_path(item.path)
            if ino is None:
                raise RuntimeError(f"Cannot apply {item}, path does not exist")
            # pyre-fixme[16]: Inode doesn't have apply_item() ...
            self._require_inode_at_path(item, item.path).apply_item(item=item)

    def apply_clone(
        self, item: SendStreamItems.clone, from_subvol: "Subvolume"
    ):
        assert isinstance(item, SendStreamItems.clone)
        # pyre-fixme[16]: Inode doesn't have apply_clone() ...
        return self._require_inode_at_path(item, item.path).apply_clone(
            item, from_subvol._require_inode_at_path(item, item.from_path)
        )

    # Exposed as a method for the benefit of `SubvolumeSet`.
    def _inode_ids_and_extents(self):
        for id, ino in self.id_to_inode.items():
            if hasattr(ino, "extent"):
                yield (id, ino.extent)

    def freeze(
        self,
        *,
        _memo,
        id_to_chunks: Optional[Mapping[InodeID, Sequence[Chunk]]] = None,
    ) -> "Subvolume":
        """
        Returns a recursively immutable copy of `self`, replacing
        `IncompleteInode`s by `Inode`s, using the provided `id_to_chunks` to
        populate them with `Chunk`s instead of `Extent`s.

        If `id_to_chunks` is omitted, we'll detect clones only within `self`.

        IMPORTANT: Our lookups assume that the `id_to_chunks` has the
        pre-`freeze` variants of the `InodeID`s.
        """
        if id_to_chunks is None:
            id_to_chunks = dict(
                extents_to_chunks_with_clones(
                    list(self._inode_ids_and_extents())
                )
            )
        return type(self)(
            id_map=freeze(self.id_map, _memo=_memo),
            id_to_inode=MappingProxyType(
                {
                    freeze(id, _memo=_memo):
                    # pyre-fixme[6]: id is Optional[InodeID] not InodeID
                    freeze(ino, _memo=_memo, chunks=id_to_chunks.get(id))
                    for id, ino in self.id_to_inode.items()
                }
            ),
        )

    def inodes(self) -> ValuesView[Union[Inode, IncompleteInode]]:
        return self.id_to_inode.values()

    def gather_bottom_up(
        self, top_path: bytes = b"."
    ) -> Coroutine[
        Tuple[
            bytes,  # full path to current inode
            Union[Inode, IncompleteInode],  # the current inode
            # None for files. For directories, maps the names of the child
            # inodes to whatever result type they had sent us.
            Optional[Mapping[bytes, Any]],
        ],  # yield
        Any,  # send -- whatever result type we are aggregating.
        Any,  # return -- the final result, whatever you sent for `top_path`
    ]:
        """
        A deterministic bottom-up traversal for aggregating results from the
        leaves of a filesystem up to a root.  Used by `render()`, but also
        good for content hashing, disk usage statistics, search, etc.

        Works on both frozen and unfrozen `Subvolume`s, you will
        correspondingly get the `Inode` or the `IncompleteInode`.

        Hardlinked files will get visited multiple times. The client can
        alway keep a map keyed on `id(ino)` to handle this.  We purposely do
        not expose `InodeID`.  A big reason to hide it is that `InodeID`s
        depend on the sequence of send-stream items that constructed the
        filesystem.  This is a problem, because one would expect a
        deterministic traversal to produce the same IDs whenever the
        underlying filesystem is the same, no matter how it was created.
        Call `.next_with_nonce(id(ino))` on a `TraversalIDMaker` to
        synthesize some filesystem-deterministic IDs for your inodes.

        Advantages over each client implementing the recursive traversal:
         - Iterative client code has simpler data flow.
         - Less likelihood that different clients traverse in different
           orders by accident.  Concretely: with manual recursion, our
           automatic `TraversalID` numbering can be either "0 at a leaf, max
           at the root", or "max at the root, 0 at a leaf", and this
           decision is implicit in the ordering of the client code.
           `gather_bottom_up` forces an explicit data flow.
         - Makes it easy to interrupt the traversal early, if appropriate.
           Note that `ctx.result` will be `None` in that case.
         - Hides our internal APIs, making it easier to later improve them.

        Usage:

            with while_not_exited(subvol.gather_bottom_up(path)) as ctx:
                result = None
                while True:
                    path, ino, child_path_to_result = ctx.send(result)
                    result = ...  # your code here
            # NB `ctx.result` will contain the result at `top_path`, which is
            # the same as `result` in this case.

        See also: `rendered_tree.gather_bottom_up()`
        """
        ino_id = self.id_map.get_id(top_path)
        assert ino_id is not None, f'"{top_path}" does not exist!'
        child_paths = self.id_map.get_children(ino_id)
        # While I'd love the next code to be a single expression using a
        # dictionary comprehension, Python does not allow `yield` inside
        # comprehensions.  See https://stackoverflow.com/questions/32139885
        if child_paths is None:
            child_results = None
        else:
            child_results = {}
            for child_path in sorted(child_paths):
                child_results[
                    os.path.relpath(child_path, top_path)
                    # pyre-fixme[7]: Expected `Coroutine[Tuple[bytes,
                    #  Union[IncompleteInode, Inode], Optional[Mapping[bytes,
                    #  typing.Any]]], typing.Any, typing.Any]` but got
                    #  `Generator[typing.Any, None, typing.Any]`.
                ] = yield from self.gather_bottom_up(child_path)
        # pyre-fixme[7]: what even?!
        return (  # noqa: B901
            # pyre-fixme[7]: Expected `Coroutine[Tuple[bytes, Union[IncompleteInode, ...
            yield (top_path, self.id_to_inode[ino_id], child_results)
        )

    def map_bottom_up(self, fn, top_path: bytes = b".") -> RenderedTree:
        """
        Applies `fn` to each inode from `top_path` down, in the
        deterministic order of `gather_bottom_up`.  Returns the results
        assembled into `RenderedTree`.
        """
        with while_not_exited(self.gather_bottom_up(top_path)) as ctx:
            result = None
            while True:
                path, ino, child_results = ctx.send(result)
                ret = fn(ino)
                # Observe that this emits `[ret, {}]` for empty dirs to
                # structurally distinguish them from files.
                result = (
                    [ret]
                    if child_results is None
                    else [
                        ret,
                        {
                            child_name.decode(
                                errors="surrogateescape"
                            ): child_result
                            for child_name, child_result in child_results.items()  # noqa: E501
                        },
                    ]
                )
        return ctx.result

    def render(self, top_path: bytes = b".") -> RenderedTree:
        """
        Produces a JSON-friendly plain-old-data view of the Subvolume.
        Before this is actually JSON-ready, you will need to call one of the
        `emit_*_traversal_ids` functions.  Read the docblock of
        `rendered_tree.py` for more details.
        """
        id_maker = TraversalIDMaker()
        return self.map_bottom_up(
            lambda ino: id_maker.next_with_nonce(id(ino)).wrap(repr(ino)),
            top_path=top_path,
        )
