#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
One of the trickier parts of creating a mock btrfs filesystem is tracking
the structures of the write forks, respecting `truncate`, `write`, and
`clone` operations.  We achieve this as follows:

 - Sequentially apply `btrfs send` operations to create & update:
    * `IncompleteInode`s and their `Extent`s,
    * the path -> `IncompleteInode` mapping.

 - Run `extents_to_chunks_with_clones()` to summarize which files clone
   which other files.  A quick clarificaiton of the notation:

    * `Extent` is actually a tree of extents, which captures the history of
      how the file's sequence of extents was created.  Refer to `extent.py`.

    * `Chunk` more directly corresponds to a filesystem extent. It's either
      data or a hole of a given length. A file is just a contiguous sequence
      of `Chunk`s.  Beyond recording the kind, and the length, each `Chunk`
      records precisely how other files clone from it.

   So `extents_to_chunks_with_clones()` flattens the history-preserving,
   clone-aware tree in `Extent` objects into a test-friendly list of
   `Chunk`s.

   For testing, it is important to produce a representation that is as
   normalized as possible: our output should deterministically and uniquely
   capture the information we wish to test, and omit everything else[1].

   We do NOT want our output to depend on the order of the operations that
   created the filesystem, but only on the final filesystem state.

   Specifically:

    * For any byte offset[2] in the file, we need to know whether it's a
      `HOLE`, or it contains `DATA` (see `Extent.Kind`).  An offset -> kind
      map is too verbose to use in manual tests, so we merge adjacent
      offsets with the same `Extent.Kind` into `Chunk`s.

    * For any offset in the file, we need to know whether it is a clone of
      any other file locations (i.e. copy-on-write sharing of underlying
      storage).  For this reason, each `Chunk` has a set of `ChunkClones`,
      which form a normalized[3] description of the shared-storage links on
      the filesystem.

      To give an example -- let's say that columns are byte offsets, and we
      have this 10-byte extent, parts of which were cloned to make files
      `A`, `B`, and `C`:

        0123456789    # offsets on disk
         BBBBBAAA     # some part of file `B` includes offsets 1-5; `A` -- 6-8
        AAACCCCC      # `A` ALSO includes 0-2, possibly separated from its 6-8

      (Aside: `test_extents_to_chunks_with_clones` also uses such figures)

      Reading this figure, we see that:

       - A has a 6-byte DATA `Chunk` with two `ChunkClones`:
          * From `offset` 1 into B at `offset` 0 with length 2, aka `B:0+2@1`
          * From `offset` 3 into C at `offset` 3 with length 2, aka `C:3+2@3'

       - B has a 5-byte DATA `Chunk` with two `ChunkClones`:
          * From `offset` 0 into A at `offset` 1 with length 2, aka `A:1+2@0`
          * From `offset` 2 into C at `offset` 0 with length 3, aka `C:0+3@2'

       - C has a 5-byte DATA `Chunk` with two `ChunkClones`:
          * From `offset` 0 into B at `offset` 2 with length 3, aka `B:2+3@0`
          * From `offset` 3 into A at `offset` 3 with length 2, aka `A:3+2@3'

      You can see that our representation of "a set of `ChunkClone`s for
      every `Chunk`" is NOT parsimonious.  If the same range of bytes is
      cloned into N `Chunk`s, each of those `Chunk`s will refer to every
      other `Chunk`, for a total of N*(N-1)/2 references.  This is far less
      efficient than a spanning tree with `N - 1` references.

      E.g. in the above example, N = 4, and we stored 6 `ChunkClones`:

        {'A': {'B:0+2@1', 'C:3+2@3'},
         'B': {'A:1+2@0', 'C:0+3@2'},
         'C': {'B:2+3@0', 'A:3+2@3'}}

      The redundancy is obvious, e.g. each of these pairs are mirror images:
        - 'A': 'B:0+2@1'    versus    'B': 'A:1+2@0'
        - 'A': 'C:3+2@3'    versus    'C': 'A:3+2@3'
        - 'B': 'C:0+3@2'    versus    'C': 'B:2+3@0'
      Picking one ChunkClone from each line would make a 3-edge spanning tree.

      Using an inefficient presentation is an intentional design decision.
      In most test filesystems, the copy number of any Chunk will be low, so
      the cost of enumerating all references is minimal.  The upside of this
      quadratic representation is that it is unique and simple.

      In contrast, presenting the clone structure via a spanning tree breaks
      the symmetry, and then each test author has to understand the process
      by which the N-1 spanning tree edges are selected.  It's easy to make
      such a process deterministic, but it still adds cognitive load.

[1] The current code tracks clones of HOLEs, because it makes no effort to
    ignore them.  I would guess that btrfs lacks this tracking, since such
    clones would save no space.  Once this is confirmed, it would be very
    easy to either ignore, or leave unpopulated the `chunk_clones` field for
    `Chunk` object with `kind == Extent.Kind.HOLE`.

[2] I refer to "bytes" throughout, but in actuality filesystems are
    block-oriented.  To deal with this, divide all lengths and offsets by
    your block size to get the sense of "bytes" used here.

[3] The current code does NOT merge adjacent ChunkClones that were created
    by separate `clone` operations.  This is easy to fix once it comes up in
    real applications.  Tested in `test_cannot_merge_adjacent_clones()`.

"""
# Future: frozentypes instead of NamedTuples can permit some cleanups below.
import functools
from collections import defaultdict
from typing import Dict, Iterable, NamedTuple, Sequence, Tuple

from antlir.btrfs_diff.extent import Extent
from antlir.btrfs_diff.inode import Chunk, ChunkClone, Clone
from antlir.btrfs_diff.inode_id import InodeID


class _CloneExtentRef(NamedTuple):
    """
    Connects a part of a HOLE/DATA leaf Extent to a location in an Inode.

    Although the Extent is shared between many inodes and/or disjoint
    locations in the same inode, each _CloneExtentRef object is specific to
    one occurrence of this Extent in the `gen_trimmed_leaves` of one inode.

    We initially create a _CloneExtentRef for every piece of every inode,
    but later we only retain those have some inter-inode overlap within
    their `.extent`, thus identifying cloned chunks of inodes.

    Aside: Unlike the simplified data model in `inode.py`, the Extent's
    object identity captures the original reason that parts of some inodes
    became identified via a clone relationship.  We mostly use this for
    assertions.

    Future: With `frozentype`, __new__ could assert that `offset` and
    `clone.length` are sane with respect to `extent`.
    """

    clone: Clone  # `clone.length` trims `extent`
    extent: Extent
    offset: int  # Trims `extent`
    # The position in `gen_trimmed_leaves` of the specific trimmed leaf that
    # is being connected to another inode.
    #
    # It is possible for a Inode to have two instances of the same Extent
    # with the same offset & length in its `gen_trimmed_leaves` stream, see
    # e.g.  `test_multi_extent`.  In that case, we cannot correctly assign
    # `ChunkClone`s to their trimmed leaves solely based on the content of
    # the trimmed leaf: `(offset, length, extent)`.
    #
    # You might ask why the `ChunkClone` lists would differ between
    # identical trimmed extents?  Here is why: the first has to refer to the
    # second, but not to itself, and conversely, the second must refer to
    # the first, but not to itself.
    #
    # We could avoid this denormalization by keying `CloneChunk`s on
    # `(inode_offset, offset, length, extent)`, which is unique.  And
    # `extents_to_chunks_with_clones` does already track `inode_offset`.
    # However, the denormalized approach seemed cleaner.
    leaf_idx: int

    def __repr__(self):  # pragma: no cover
        return (
            f"{self.clone.inode_id}:{self.clone.offset}"
            f"+{self.clone.length}:{id(self.extent)}"  # Extent is too noisy
        )


# If these change, we have to update `_clone_op_compare_key`
assert Clone._fields.index("inode_id") == 0
assert _CloneExtentRef._fields.index("clone") == 0


# Our _CloneOp ordering obeys the following invariants:
#  - sort by position first
#  - sort by action second, putting POPs before PUSHes (see their def'ns)
# We do not need finer-grained ordering because:
#  (1) we only do work on POPs,
#  (2) the work done on all the POPs at one position does not depend on the
#      order of the _CloneOps -- we symmetrically record the relationship in
#      both directions:
#        (just-popped op, each unpopped op)
#        (each unpopped op, just-popped op)
#
# We could get the desired ordering implicitly by:
#  - relying on the order of field declaration in `_CloneOp` (not bad)
#  - making `Inode`s comparable (a bit ugly, comparing Extents is pricy,
#    comparing InodeIDs would require some comparator boilerplate)
# Luckily, being explicit is not *that* painful.
def _clone_op_compare_key(c: "_CloneOp"):
    return (
        # The preceding asserts make these [1:] hacks tolerable.
        c.pos,
        c.action,
        c.ref[1:],
        c.ref.clone[1:],
        c.ref.clone.inode_id.id,
    )


def _clone_op_compare(fn):
    @functools.wraps(fn)
    def cmp(self: "_CloneOp", other: "_CloneOp"):
        assert isinstance(other, _CloneOp)
        # We only compare ops within one extent. The tests assume this to
        # justify focusing on single-extent examples, so check it.
        assert self.ref.extent is other.ref.extent
        # All our items are distinct, since `clone.offset` is `inode_offset`,
        # which is strictly increasing in each inode.  We have no business
        # comparing a _CloneOp with itself.
        assert tuple.__ne__(self, other)
        return fn(_clone_op_compare_key(self), _clone_op_compare_key(other))

    return cmp


class _CloneOp(NamedTuple):
    PUSH = "push"
    POP = "pop"
    assert POP < PUSH  # We want to sort all POPs before any PUSHes

    pos: int
    action: str
    ref: _CloneExtentRef

    # NamedTuple confuses functools.total_ordering, so define all 6 comparators
    __eq__ = _clone_op_compare(tuple.__eq__)
    __ne__ = _clone_op_compare(tuple.__ne__)
    __lt__ = _clone_op_compare(tuple.__lt__)
    __le__ = _clone_op_compare(tuple.__le__)
    __gt__ = _clone_op_compare(tuple.__gt__)
    __ge__ = _clone_op_compare(tuple.__ge__)


def _leaf_extent_id_to_clone_ops(
    ids_and_extents: Iterable[Tuple[InodeID, Extent]]
):
    """
    To collect the parts of a Chunk that are cloned, we will run a variation
    on the standard interval-overlap algorithm.  We first sort the starts &
    ends of each interval, and then do a sequential scan that uses starts to
    add, and ends to remove, a tracking object from a "current intervals"
    structure.

    This function simply prepares the set of interval starts & ends for each
    InodeID, the computation is in `_leaf_ref_to_chunk_clones_from_clone_ops`.
    """
    leaf_extent_id_to_clone_ops = defaultdict(list)
    for ino_id, extent in ids_and_extents:
        file_offset = 0
        for leaf_idx, (offset, length, leaf_extent) in enumerate(
            extent.gen_trimmed_leaves()
        ):
            ref = _CloneExtentRef(
                clone=Clone(inode_id=ino_id, offset=file_offset, length=length),
                extent=leaf_extent,
                offset=offset,
                leaf_idx=leaf_idx,
            )
            leaf_extent_id_to_clone_ops[id(leaf_extent)].extend(
                [
                    _CloneOp(pos=offset, action=_CloneOp.PUSH, ref=ref),
                    _CloneOp(pos=offset + length, action=_CloneOp.POP, ref=ref),
                ]
            )
            file_offset += length
    return leaf_extent_id_to_clone_ops


def _leaf_ref_to_chunk_clones_from_clone_ops(
    extent_id: int, clone_ops: Iterable[_CloneOp]
):
    "As per `_leaf_extent_id_to_clone_ops`, this computes interval overlaps"
    active_ops: Dict[_CloneExtentRef, _CloneOp] = {}  # Tracks open intervals
    leaf_ref_to_chunk_clones = defaultdict(list)
    for op in sorted(clone_ops):
        # Whenever an interval (aka an Inode's Extent's "trimmed leaf")
        # ends, we create `ChunkClone` objects **to** and **from** all the
        # concurrently open intervals.
        if op.action is _CloneOp.POP:
            pushed_op = active_ops.pop(op.ref)
            assert pushed_op.ref is op.ref
            assert id(op.ref.extent) == extent_id
            assert pushed_op.pos == op.ref.offset
            assert pushed_op.pos + op.ref.clone.length == op.pos

            for clone_op in active_ops.values():
                assert op.ref.extent is clone_op.ref.extent

                # The cloned portion's extent offset is the larger of the 2
                bigger_offset = max(clone_op.ref.offset, op.ref.offset)

                # Record that `clone_op` clones part of `op`'s inode.
                leaf_ref_to_chunk_clones[op.ref].append(
                    ChunkClone(
                        offset=bigger_offset,
                        clone=Clone(
                            inode_id=clone_op.ref.clone.inode_id,
                            offset=clone_op.ref.clone.offset
                            + (bigger_offset - clone_op.ref.offset),
                            length=op.pos - bigger_offset,
                        ),
                    )
                )

                # Record that `op` clones part of `clone_op`'s inode.
                leaf_ref_to_chunk_clones[clone_op.ref].append(
                    ChunkClone(
                        offset=bigger_offset,
                        clone=Clone(
                            inode_id=op.ref.clone.inode_id,
                            offset=op.ref.clone.offset
                            + (bigger_offset - op.ref.offset),
                            length=op.pos - bigger_offset,  # Same length
                        ),
                    )
                )
        # Sorting guarantees all POPs for `pos` are handled before PUSHes
        elif op.action == _CloneOp.PUSH:
            assert op.ref not in active_ops
            active_ops[op.ref] = op
        else:
            raise AssertionError(op)  # pragma: no cover
    return leaf_ref_to_chunk_clones


def _id_to_leaf_idx_to_chunk_clones(
    ids_and_extents: Iterable[Tuple[InodeID, Extent]]
):
    'Aggregates newly created ChunkClones per InodeID, and per "trimmed leaf"'
    id_to_leaf_idx_to_chunk_clones = defaultdict(dict)
    for extent_id, clone_ops in _leaf_extent_id_to_clone_ops(
        ids_and_extents
    ).items():
        leaf_ref_to_chunk_clones = _leaf_ref_to_chunk_clones_from_clone_ops(
            extent_id, clone_ops
        )
        for leaf_ref, offsets_clones in leaf_ref_to_chunk_clones.items():
            d = id_to_leaf_idx_to_chunk_clones[leaf_ref.clone.inode_id]
            # A `leaf_idx` from a specific inode ID refers to one extent,
            # and each extent is handled in one iteration, so it cannot be
            # that two iterations contribute to the same `leaf_idx` key.
            assert leaf_ref.leaf_idx not in d
            # `leaf_idx` is the position in `gen_trimmed_leaves` of the
            # chunk, whose clones we computed.  That fully specifies where
            #  `extents_to_chunks_with_clones` should put the clones.
            d[leaf_ref.leaf_idx] = offsets_clones

    return id_to_leaf_idx_to_chunk_clones


def extents_to_chunks_with_clones(
    ids_and_extents: Sequence[Tuple[InodeID, Extent]]
) -> Iterable[Tuple[InodeID, Sequence[Chunk]]]:
    """
    Converts the nested, history-preserving `Extent` structures into flat
    sequences of `Chunk`s, while being careful to annotate cloned parts as
    described in this file's docblock.  The `InodeID`s are needed to ensure
    that the `Chunk`s' `Clone` objects refer to the appropriate files.
    """
    id_to_leaf_idx_to_chunk_clones = _id_to_leaf_idx_to_chunk_clones(
        ids_and_extents
    )
    for ino_id, extent in ids_and_extents:
        leaf_to_chunk_clones = id_to_leaf_idx_to_chunk_clones.get(ino_id, {})
        new_chunks = []
        for leaf_idx, (offset, length, extent) in enumerate(
            extent.gen_trimmed_leaves()
        ):
            chunk_clones = leaf_to_chunk_clones.get(leaf_idx, [])
            assert isinstance(extent.content, Extent.Kind)

            # If the chunk kind matches, merge into the previous chunk.
            if new_chunks and new_chunks[-1].kind == extent.content:
                prev_length = new_chunks[-1].length
                prev_clones = new_chunks[-1].chunk_clones
            else:  # Otherwise, make a new one.
                prev_length = 0
                prev_clones = set()
                new_chunks.append(None)

            new_chunks[-1] = Chunk(
                kind=extent.content,
                length=length + prev_length,
                chunk_clones=prev_clones,
            )
            new_chunks[-1].chunk_clones.update(
                # Future: when switching to frozentype, __new__ should
                # validate that clone offset & length are sane relative
                # to the trimmed extent.
                ChunkClone(
                    clone=clone,
                    # Subtract `offset` because `ChunkClone.offset` is
                    # Extent-relative, but in the actual file layout, the
                    # leaf Extent is trimmed further.
                    offset=clone_offset + prev_length - offset,
                )
                for clone_offset, clone in chunk_clones
            )
        # Future: `deepfrozen` was made for this:
        yield ino_id, tuple(
            Chunk(
                kind=c.kind,
                length=c.length,
                # pyre-fixme[6]: Expected `Set[ChunkClone]` for 3rd param but got
                #  `frozenset[Variable[_T_co](covariant)]`.
                chunk_clones=frozenset(c.chunk_clones),
            )
            for c in new_chunks
        )
