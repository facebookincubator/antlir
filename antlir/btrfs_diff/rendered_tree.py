#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A `RenderedTree` is a JSON-friendly plain-old-data view of the `Subvolume`.

This a recursive structure of the form `[inode, {'name': [...]}]`. Arbitrary
inode representations are allowed.  The format is meant for humans to write,
to consume, and to manipulate with standard tools like `jq` and `python`.

We do NOT call this a "serialization" because it is not meant to be parsed
my computers in a lossless fashion.  Rather it is meant to be a concise,
human-readable representation, which captures only the aspects of the
subvolume tree that the user considers important.

The regular flow for producing a `RenderedTree` has two steps:

  `Subvolume.render` --> `RenderedTree` with `TraversalIDWrapper`s -->
      --> `emit_*_traversal_id` -> JSON-friendly `RenderedTree`

This offers some flexibility in how you express references to one object
(e.g. inode hardlinks) occurring at different points in the tree, see (2).

Since `RenderedTree` is plain-old-data (not a class), this module instead
offers helpers for operating on the data structure: `map_bottom_up` for
rewriting `RenderedTrees`, and the underlying traversal `gather_bottom_up`.

There are two aspects to rendering a `Subvolume`:

(1) The structure of the `RenderedTree` should permit storing arbitrary
representations of inodes.  At the same time, for post-processing, we must
be able to unambiguously traverse its paths & inodes, whatever their format.
Logically, a tree is:
  - either an `inode` together with `{'child_name': tree, ...}` -- a directory,
  - or just an `inode` -- a file.
Since `inode`s can be arbitrary structures, the `tree` should externally
identify the inode by its position in the structure. Our solution is:
   - `[inode, {...}]` for directories,
   - `[inode]` for files.

(2) When the same object occurs multiple times in the tree, we need a way of
expressing this aliasing in our JSON-friendly plain-old-data serialization.
From here on out, I'll talk about inodes and hardlinks, but this approach
can also be adapted to cloned extents.  The tried-and-true approach is to
number all distinct objects (i.e.  inode numbers), so the IDs of any aliased
objects will appear more than once.

For example, `['(Dir)': {'a': ['(File)'], 'a': ['(File)']]` will become:
  (i) if 'a' and 'b' are distinct inodes:
      `[['(Dir)', 2]: {'a': [['(File)', 0]], 'a': [['(File)', 1]]]`
 (ii) if 'a' and 'b' are the same inode:
      `[['(Dir)', 1]: {'a': [['(File)', 0]], 'a': [['(File)', 0]]]`
Note that we numbered from the bottom up, with the children traversed in
lexicographic order.  This is the traversal order of `gather_bottom_up` and
of `Subvolume.{gather_bottom_up,render}`.

This numbering mechanism is implemented by `TraversalIDMaker` paired with
`emit_all_traversal_ids`.

Working with exhaustive numbering gets very tedious if you are have anything
but the tiniest filesystem.  For this reason, `TraversalID` operates by
initially wrapping the object it identifies, and **in a second pass**
emitting a JSON-friendly representation.  This allows us to have
`emit_non_unique_traversal_ids`, which leaves unique objects as-is, and
wraps only objects that occur more than once. For example, (ii) becomes:

 (ii) if 'a' and 'b' are the same inode:
      `['(Dir)': {'a': [['(File)', 0]], 'a': [['(File)', 0]]]`
"""
import os
from itertools import count
from typing import (
    Any,
    Coroutine,
    Hashable,
    Mapping,
    NamedTuple,
    Optional,
    Tuple,
    Union,
)

from antlir.btrfs_diff.coroutine_utils import while_not_exited


# This is intended to be:
# RenderedTree = Union[Tuple[Any], Tuple[Any, Mapping[bytes, 'RenderedTree']]]
# But recursive types don't work for now so YOLO
RenderedTree = Union[Tuple[Any], Tuple[Any, Mapping[bytes, Any]]]


def gather_bottom_up(
    ser: RenderedTree,
    *,
    # Private, and NOT like the public `top_path` arg of `gather_bottom_up`.
    # Not `bytes` since we `surrogateescape` everything at render time to
    # let us produce JSON-friendly `utf-8`.
    _path: str = ".",
) -> Coroutine[
    Tuple[
        bytes,  # full path to current inode
        Any,  # the current inode
        # None for files. For directories, maps the names of the child
        # inodes to whatever result type they had sent us.
        Optional[Mapping[bytes, Any]],
    ],  # yield
    Any,  # send -- whatever result type we are aggregating.
    Any,  # return -- the final result, whatever you sent for `top_path`
]:
    """
    A deterministic bottom-up traversal over the inodes in the output of
    `Subvolume.render`.  This matches the traversal order of
    `Subvolume.gather_bottom_up`.  See that docblock for a discussion of the
    merits of traversal coroutines.
    """
    if not isinstance(ser, list):
        raise RuntimeError(f"Unknown type in rendered subvolume: {ser}")
    elif len(ser) == 1:
        return (yield _path, ser[0], None)
    elif len(ser) != 2:
        raise RuntimeError(f"Rendered inode list length != 1, 2: {ser}")

    ino, children = ser
    # Normally, we'd just get a 1-element list, but this is OK too.
    if children is None:
        child_results = None
    else:
        child_results = {}
        # Traverse children in the same order as `gather_bottom_up`,
        # ensuring that in tests actual & expected traversal IDs agree.
        for name, child_ser in sorted(children.items()):
            child_results[name] = yield from gather_bottom_up(
                child_ser,
                # normpath to remove the leading ./
                _path=os.path.normpath(os.path.join(_path, name)),
            )
    return (yield (_path, ino, child_results))  # noqa: B901


def map_bottom_up(ser: RenderedTree, fn) -> RenderedTree:
    """
    Like `gather_bottom_up`, but applies `fn` to all the inodes and produces
    a new a `RenderedTree` with the results.  The only downside is that
    inodes cannot see their children.
    """
    with while_not_exited(gather_bottom_up(ser)) as ctx:
        result = None
        while True:
            path, old_ino, children = ctx.send(result)
            new_ino = fn(old_ino)
            result = [new_ino] if children is None else [new_ino, children]
    return ctx.result


class TraversalIDWrapper(NamedTuple):
    """
    Do not construct this directly -- instead call `TraversalID.wrap`.
    If you see this in your output, call `emit_*_traversal_ids`.

    Making JSON-friendly output is a 2-stage process. First, we
    produce the inode representations, wrapped with `TraversalIDWrapper`.
    Then, `emit_*_traveral_ids` replaces the wrappers with JSON-friendly
    annotations that identify any repeated objects.
    """

    id: "TraversalID"
    wrapped: Any


class TraversalID:
    """
    `TraversalIDMaker` exists to highlight which objects are the same.
    Using a refcounting `TraveralID` instead of a bare `int` enables us to
    show IDs only when the object is actually repeated.
    """

    id: int
    refcount: int

    def __init__(self, id: int) -> None:
        self.id = id
        self.refcount = 0

    # `__eq__` and `__hash__` are stubbed out because this class, if you
    # were to use it as a key, must have value semantics (i.e. it should
    # act as the tuple `(self.id, self.refcount)` or perhaps even as just
    # `self.id`).  Without these stubs, it would get the default semantics
    # of "hash/compare `id(self)`".
    #
    # At present, I consider it a bug to use `TraversalID` directly.
    # Instead, you should call an `emit_*` function on the `RenderedTree`,
    # and operate with the resulting plain-old-data structure.

    def __eq__(self, other):  # pragma: no cover
        raise NotImplementedError

    def __hash__(self):  # pragma: no cover
        raise NotImplementedError

    def __repr__(self) -> str:
        # This is verbose because we shouldn't use `TraversalID`s directly,
        # but should rather always emit them to finish the rendering.
        return f"TraversalID({self.id}/{self.refcount})"

    def wrap(self, wrapped: Any) -> TraversalIDWrapper:
        return TraversalIDWrapper(id=self, wrapped=wrapped)


class TraversalIDMaker:
    """
    If we traverse a filesystem in a deterministic order, and increment a
    counter every time we encounter a previously-unseen object (e.g.  a new
    inode or a new data extent), we will end up with **deterministic** IDs
    that capture the object-aliasing structure of the filesystem.  That is,
    IDs emitted in different parts of the traversal will be the same iff the
    traversal is visiting the same object twice (e.g.  hardlinks or cloned
    extents).
    """

    def __init__(self) -> None:
        self.counter = count()
        self.nonce_to_id = {}

    def next_with_nonce(self, nonce: Hashable) -> TraversalID:
        """
        Call this for each object you encounter in your traversal.

        The nonce is any key that captures your desired meaning of object
        identity.  Often, you will just use `id(obj)`.  Caveat: For
        value-is-identity objects like `InodeID`, it is better to encode the
        value in the key, e.g.  `(ino_id.id, id(ino_id.inner_id_map))` makes
        more sense than `id(ino_id)`.
        """
        if nonce not in self.nonce_to_id:
            self.nonce_to_id[nonce] = TraversalID(next(self.counter))
        trav_id = self.nonce_to_id[nonce]
        trav_id.refcount += 1
        return trav_id

    def next_unique(self) -> TraversalID:
        "Use this to make test data where most objects are unique."
        return self.next_with_nonce(object())


def emit_all_traversal_ids(ser: RenderedTree) -> RenderedTree:
    "Every inode becomes [ino, integer traversal ID]."
    return map_bottom_up(
        ser, lambda wrapped_ino: [wrapped_ino.wrapped, wrapped_ino.id.id]
    )


def emit_non_unique_traversal_ids(ser: RenderedTree):
    """
    Inodes that occur more than once become `[ino, integer ID]`, with
    new, sequential, deterministic IDs just for the non-unique inodes.
    Unique inodes are emitted as `ino`.
    """
    id_maker = TraversalIDMaker()

    def maybe_emit_id(wrapped_ino):
        if wrapped_ino.id.refcount < 2:
            return wrapped_ino.wrapped
        return [
            wrapped_ino.wrapped,
            id_maker.next_with_nonce(wrapped_ino.id.id).id,
        ]

    return map_bottom_up(ser, maybe_emit_id)
