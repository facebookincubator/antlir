#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A `SubvolumeSet` maps path subtrees to `Subvolume`s. Therefore, it only
knows how to apply the initial `SendStreamItems` types that set the
subvolume for the rest of the stream.

Note that at present, `Subvolume`s are **not** mounted into a shared
directory tree the way they would be in a real btrfs filesystem.  This is
not done here simply because we don't have a need to model it, but you can
easily imagine a path-aware `Volume` abstraction on top of this.
"""
import copy
import itertools
from collections import Counter
from types import MappingProxyType

# Future: `deepfrozen` would let us lose the `new` methods on NamedTuples,
# and avoid `deepcopy`.
from typing import Iterator, Mapping, NamedTuple, Optional, Union

from antlir.btrfs_diff.extents_to_chunks import extents_to_chunks_with_clones
from antlir.btrfs_diff.freeze import freeze
from antlir.btrfs_diff.incomplete_inode import IncompleteInode
from antlir.btrfs_diff.inode import Inode
from antlir.btrfs_diff.inode_id import InodeIDMap
from antlir.btrfs_diff.rendered_tree import RenderedTree
from antlir.btrfs_diff.send_stream import SendStreamItem, SendStreamItems
from antlir.btrfs_diff.subvolume import Subvolume


class SubvolumeID(NamedTuple):
    uuid: str
    # NB: in principle, we might want to check that the transaction ID
    # matches that of the `clone` command, to increase the odds that we are
    # cloning the bits we expect to be cloning.  However, at the time of
    # writing, `btrfs-progs` does not perform this check.  To add this check
    # here, we'd need to make sure that the transaction ID is meaningfully
    # updated as a send-stream is applied.  This seems problematic, since
    # the organically obtained transaction ID on the source volume need not
    # have any correspondence to the number of transactions encoded in the
    # send-stream -- a send-stream might encode the same filesystem changes
    # in fewer or more transactions than did the underlying VFS commands.
    # Therefore, the only context in which it is meaningful to check
    # transaction IDs is when the parent was built up from the same exact
    # sequence of diffs on both the sending & the receiving side.  Achieving
    # this would involve re-applying each diffs at build-time, which besides
    # code complexity may incur some performance overhead.
    transid: int


class SubvolumeDescription(NamedTuple):
    """
    This is a "cheat" to make debugging & testing easier, but it is NOT part
    of the core data model.  If you are caught using it in real business
    logic, you will be asked to wear a red nose and/or a cone of shame.

    In particular, these fields are deliberately NOT on `Subvolume` because
    only `SubvolumeSet` should know the context in which the `Subvolume`
    exists.

    We give store this as `InodeIDMap.description` to make it easy to
    distinguish between `InodeID`s from different `Subvolume`s.

    IMPORTANT: Because of our `.name_uuid_prefix_counts` member, which is
    owned by a `SubvolumeSet`, this object would ONLY be safely
    `deepcopy`able if we were to copy the `SubvolumeSet` in one call -- but
    we never do that.  When we make snapshots in `SubvolumeSetMutator`, we
    have to work around this problem, since a naive implementation would
    `deepcopy` this object via `Subvolume.id_map.description`.
    """

    name: bytes
    id: SubvolumeID
    parent_id: Optional[SubvolumeID]
    # See the IMPORTANT note in the docblock about this member:
    name_uuid_prefix_counts: Mapping[str, int]

    def name_uuid_prefixes(self):
        name = self.name.decode(errors="surrogateescape")
        for i in range(len(self.id.uuid) + 1):
            yield (name + "@" + self.id.uuid[:i]) if i else name

    def __repr__(self):
        for prefix in self.name_uuid_prefixes():
            if self.name_uuid_prefix_counts.get(prefix, 0) < 2:
                return prefix
        # Happens when one uuid is a prefix of another, i.e. in tests.
        return f"{prefix}-ERROR"


class SubvolumeSet(NamedTuple):
    "IMPORTANT: Keep this `deepcopy`able for the sake of tests."
    uuid_to_subvolume: Mapping[str, Subvolume]
    # `SubvolumeDescription` wants to represent itself as `name@abc`, where
    # `abc` is the shortest prefix of its UUID that uniquely identifies it
    # within the `SubvolumeSet`.  In order to make it easy to find this
    # shortest prefix, we keep track of the count of `name@uuid_prefix` for
    # each possible length of prefix (from 0 to `len(uuid)`).  When the name
    # is unique, `@uuid_prefix` is omitted (aka prefix length 0).
    name_uuid_prefix_counts: Mapping[str, int]

    @classmethod
    def new(cls, **kwargs) -> "SubvolumeSet":
        kwargs.setdefault("uuid_to_subvolume", {})
        kwargs.setdefault("name_uuid_prefix_counts", Counter())
        return cls(**kwargs)

    def get_by_rendered_id(self, rendered_id: str) -> Optional[Subvolume]:
        for subvol in self.uuid_to_subvolume.values():
            if repr(subvol.id_map.inner.description) == rendered_id:
                return subvol
        return None

    def freeze(self, *, _memo) -> "SubvolumeSet":
        """
        Return a recursively immutable copy of `self`, replacing all
        `IncompleteInode`s by `Inode`s, and checking that all inode metadata
        are populated.  Correctly resolving cloned extents has to happen at
        the level of the `SubvolumeSet`.
        """
        id_to_chunks = dict(
            extents_to_chunks_with_clones(
                list(
                    itertools.chain.from_iterable(
                        subvol._inode_ids_and_extents()
                        for subvol in self.uuid_to_subvolume.values()
                    )
                )
            )
        )
        return type(self)(
            uuid_to_subvolume=MappingProxyType(
                {
                    uuid: freeze(subvol, _memo=_memo, id_to_chunks=id_to_chunks)
                    for uuid, subvol in self.uuid_to_subvolume.items()
                }
            ),
            name_uuid_prefix_counts=freeze(self.name_uuid_prefix_counts, _memo=_memo),
        )

    def inodes(self) -> Iterator[Union[Inode, IncompleteInode]]:
        return itertools.chain.from_iterable(
            sv.inodes() for sv in self.uuid_to_subvolume.values()
        )

    def map(self, fn) -> Mapping[str, RenderedTree]:
        """
        Applies `fn` to each subvolume. Returns results indexed by a string
        containing the subvolume name and the minimal unambiguous prefix of
        its UUID (in the style of `git` or `hg`).

        NB: If you map over an un-frozen SubvolumeSet, you will get
        `IncompleteInode`s, which are not aware of cloned extents.
        """
        return {
            repr(subvol.id_map.inner.description): fn(subvol)
            for subvol in self.uuid_to_subvolume.values()
        }


class SubvolumeSetMutator(NamedTuple):
    """
    A send-stream always starts with a command defining the subvolume,
    to which the remaining stream commands will be applied.  Since
    `SubvolumeSet` is only responsible for managing `Subvolume`s, this
    is essentially a proxy for `Subvolume.apply_item`.

    The reason we don't just return `Subvolume` to the caller after
    the first item is that we need some logic as the `SubvolumeSet` and
    `Subvolume` layers to resolve `clone` commands.
    """

    subvolume: Subvolume
    subvolume_set: SubvolumeSet

    @classmethod
    def new(
        cls, subvol_set: SubvolumeSet, subvol_item: SendStreamItem
    ) -> "SubvolumeSetMutator":
        if not isinstance(
            subvol_item, (SendStreamItems.subvol, SendStreamItems.snapshot)
        ):
            raise RuntimeError(f"{subvol_item} must specify subvolume")

        my_id = SubvolumeID(uuid=subvol_item.uuid.decode(), transid=subvol_item.transid)
        parent_id = (
            SubvolumeID(
                uuid=subvol_item.parent_uuid.decode(),
                transid=subvol_item.parent_transid,
            )
            if isinstance(subvol_item, SendStreamItems.snapshot)
            else None
        )
        description = SubvolumeDescription(
            name=subvol_item.path,
            id=my_id,
            parent_id=parent_id,
            name_uuid_prefix_counts=subvol_set.name_uuid_prefix_counts,
        )
        if isinstance(subvol_item, SendStreamItems.snapshot):
            uuid = parent_id.uuid if parent_id is not None else ""
            parent_subvol = subvol_set.uuid_to_subvolume[uuid]
            # `SubvolumeDescription` references a part `SubvolumeSet`, so it
            # is not correctly `deepcopy`able as part of a `Subvolume`.  And
            # we want to modify the `InodeIDMap`'s `description` in any
            # case, so let's just bulk-replace the old description instance.
            # This would not be sane if the old instance were of a type that
            # may be interned by the runtime, like `int`, hence the assert.
            assert isinstance(
                parent_subvol.id_map.inner.description, SubvolumeDescription
            )
            subvol = copy.deepcopy(
                parent_subvol,
                memo={id(parent_subvol.id_map.inner.description): description},
            )
        else:
            subvol = Subvolume.new(id_map=InodeIDMap.new(description=description))

        dup_subvol = subvol_set.uuid_to_subvolume.get(my_id.uuid)
        if dup_subvol is not None:
            raise RuntimeError(f"{my_id} is already in use: {dup_subvol}")
        # pyre-fixme[16]: This is supposed to be frozen!!!
        subvol_set.uuid_to_subvolume[my_id.uuid] = subvol

        # insertion can fail, so update the description disambiguator last.
        # pyre-fixme[16]: This is supposed to be frozen!!!
        subvol_set.name_uuid_prefix_counts.update(description.name_uuid_prefixes())

        return cls(subvolume=subvol, subvolume_set=subvol_set)

    def apply_item(self, item: SendStreamItem):
        if isinstance(item, SendStreamItems.clone):
            from_subvol = self.subvolume_set.uuid_to_subvolume.get(
                item.from_uuid.decode()
            )
            if not from_subvol:
                raise RuntimeError(f"Unknown from_uuid for {item}")
            return self.subvolume.apply_clone(item, from_subvol)
        return self.subvolume.apply_item(item)
