#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import enum
import itertools
from typing import Iterable, List, NamedTuple, Optional, Tuple, Union


class Action(enum.Enum):
    _RECURSE = "recurse"
    _YIELD = "yield"


def _get_trimmed_leaves(
    extent: "Extent", offset: int = 0, length: Optional[int] = None
) -> Tuple[
    Action,
    Union[
        Tuple[int, int, "Extent"],  # _YIELD case
        List[Tuple["Extent", int, int]],  # _RECURSE case
    ],
]:
    max_length = extent.length - offset
    if length is None:
        length = max_length
    # These don't print `extent` since that would recurse into this func
    assert length <= max_length, f"len {length}, offset {offset}"
    assert offset >= 0 and length >= 0, f"offset {offset}, length {length}"
    assert offset <= extent.length, f"offset {offset}"

    maybe_content = extent.content
    if isinstance(maybe_content, Extent.Kind):
        trimmed_length = min(length, extent.length - offset)
        if trimmed_length > 0:
            return Action._YIELD, (offset, trimmed_length, extent)

    # trimmed_length <= 0 should not happen in well-formed input
    assert not isinstance(
        maybe_content, Extent.Kind
    ), f"min({length}, {extent.length} - {offset}) <= 0"

    offset += extent.offset
    recurse_list = []
    for e in maybe_content:
        if e.length > offset:
            recurse_list.append((e, offset, min(length, e.length - offset)))
            length -= e.length - offset
            if length <= 0:
                break
        offset -= min(offset, e.length)
    return Action._RECURSE, recurse_list


# Future: use `deepfrozentype` for true immutability.
class Extent(NamedTuple):
    """
    An Extent represents a contiguous sequence of bytes, which were either:
     - written (`write`)
     - cloned from another extent (btrfs clone `ioctl`)
     - deliberately left as holes (`truncate`)

    A file always has one Extent object, and it is placed at file offset 0.
    Holes are modeled explicitly via subextents.

    `Extent`s are recursively immutable. This lets us identify cloned parts
    of files by simply checking `Extent` object identity (via `is` or `id`).

    To modify a file, just replace its `Extent` with whatever the mutator
    method returned.

    `copy` / `deepcopy` alert: clone identification relies on object
    identity, so copying `Extent`s would be a great way to subtly discard
    clone information.  Luckily, our objects are recursively immutable,
    making it safe to have both copy mechanisms return simply `self`.

    Users should NOT directly create extents. Also, the fields of `offset`
    and `content` cannot be usefully introspected.
     - Start each file with `empty()`
     - Use the `btrfs send` workalikes of `truncate()`, `write()`, `clone()`
       to change files.
     - Use `.length`, and `gen_trimmed_leaves()` to inspect the objects.

    ## KEY INTERNAL INVARIANT

    Extent operations will NEVER replace ground-truth extents (ones with
    `content` of type `Extent.Kind`, aka tree leaves) by new objects.
    Neither construction-time optimizations, nor traversals will do this.

    To illustrate, suppose an optimization wants to to truncate 1 byte off
    the front & back of:
        Extent(content=Extent.Kind.DATA, offset=0, length=5)
    One way would be to create a new, smaller DATA extent.
        Extent(content=Extent.Kind.DATA, offset=0, length=3)
    This is BAD, because it discards object identity between the new and
    old.  The GOOD approach is to wrap the original object:
        Extent(
            Extent(content=Extent.Kind.DATA, offset=0, length=5),
            offset=1,
            length=3,
        )

    This greatly simplifies clone tracking. It lets `gen_trimmed_leaves`
    flatten Extents to one level of nesting: a list of a trimmed DATA or
    HOLE extents.  Since the ground-truth extents have the same object
    identity from their time of creation, this retains all the necessary
    information for deciding if two normalized extents share cloned bytes.

    ## Design note

    For the purposes of write/clone/truncate modeling, `Extent` could do a
    lot more on-the-fly normalization, which could save RAM.  E.g. we could
    discard HOLE provenance (just store "hole of length N"), and we could
    flatten to trimmed leaves more eagerly.  However, none of this extra
    complexity seems worthwhile for now.

    """

    content: Union["Extent.Kind", Iterable["Extent"]]
    offset: int
    length: int

    class Kind(enum.Enum):
        DATA = 1
        HOLE = 2

        # The default `__repr__` was not `eval`able :/
        def __repr__(self):
            # Assume every reasonable user will have `Extent` in their scope
            return f"Extent.Kind.{self.name}"

    # NB: we do not want this to be `__new__`, since it is allowed **NOT**
    # to return a new object, but instead to extract one from `content`.
    @staticmethod
    def __new(
        # IMPORTANT: `Extent` may rewrite its inputs before storing them in
        # the object, so do not try to read them back.  Example: if `offset`
        # & `length` are such that certain subextents are inaccessible, we
        # will discard those subextents, and reduce `offset` accordingly.
        content: Union["Extent.Kind", Iterable["Extent"], "Extent"],
        *,
        # Hide the first `offset` bytes of the constituent extents.
        offset: int = 0,
        # Hide the bytes of the consitutent extents past `offset + length`.
        # Inferred from the constituent extents if omitted.
        length: Optional[int] = None,
    ):
        if isinstance(content, Extent):  # Avoid writing (self,) everywhere
            content = (content,)
        # Runtime enforcement because I don't trust my coding :)
        assert isinstance(content, Extent.Kind) or (
            isinstance(content, tuple) and all(isinstance(e, Extent) for e in content)
        ), f"Invalid extent content: {content}"
        # If you hit this, you should create a HOLE followed by DATA.
        assert (
            isinstance(content, tuple) or offset == 0
        ), "Nonzero offsets only make sense for extents-of-extents"
        assert offset >= 0, f"offset {offset} < 0"

        # Computed length can be negative -- e.g. this happens if a
        # `write()` affects bytes past the end of the current extent.
        # This is OK, and we want to treat this extent as empty.
        max_length = (
            max(0, sum(e.length for e in content) - offset)
            if isinstance(content, tuple)
            else None
        )

        if length is None:
            assert max_length is not None, f"{content} must specify length"
            length = max_length
        length = max(0, length)  # e.g. the `truncate` HOLE length may be < 0

        # If the caller needs to add a HOLE at the end, it has to be explicit.
        assert (
            length is None or max_length is None or max_length >= length
        ), f"Computed max length {max_length} > specified {length}"

        if isinstance(content, tuple):

            def optimize_content():
                "Drop hidden and 0-length extents"
                nonlocal offset  # Will be reduced if we skip initial extents
                new_content_len = 0
                for e in content:
                    if e.length == 0:
                        continue  # Skip empty extents
                    if e.length <= offset:
                        offset -= e.length
                        continue  # Skip extents that precede `offset`
                    if new_content_len >= offset + length:
                        continue  # Skip extents that follow `offset + length`
                    new_content_len += e.length
                    yield e

            content = tuple(optimize_content())

        # Elide nesting when we end up with the identity transform on 1 extent
        if (
            isinstance(content, tuple)
            and len(content) == 1
            and offset == 0
            and length == content[0].length
        ):
            return content[0]

        return Extent(content=content, offset=offset, length=length)

    @staticmethod
    def empty():
        return Extent.__new(())

    def truncate(self, length: int):
        return Extent.__new(
            (self, Extent.__new(Extent.Kind.HOLE, length=length - self.length)),
            length=length,
        )

    def __put(self, offset: int, what: "Extent"):
        "Overwrites with `what` a portion of `self` starting at `offset`."
        # E.g., should `extent.Extent.empty().write(offset=5, length=0)`
        # create a hole, or remain empty?
        assert what.length > 0, "Future: not sure how to hangle length = 0"
        return Extent.__new(
            (
                Extent.__new(self, length=min(self.length, offset)),
                Extent.__new(Extent.Kind.HOLE, length=offset - self.length),
                what,
                Extent.__new(self, offset=(offset + what.length)),
            )
        )

    def write(self, *, offset: int, length: int):
        return self.__put(offset, Extent.__new(Extent.Kind.DATA, length=length))

    def clone(
        self,
        *,
        to_offset: int,
        from_extent: "Extent",
        from_offset: int,
        length: int,
    ):
        return self.__put(
            to_offset,
            Extent.__new(from_extent, offset=from_offset, length=length),
        )

    def gen_trimmed_leaves(self, *, offset: int = 0, length: Optional[int] = None):
        """
        Yields the sequence of
           (offset, length, leaf subextent with Extent.Kind content),
        which you would witness on a filesystem, if the operations in `self`
        were actually executed.

        Caveat: `offset` and `length` are instructions for how to trim the
        subextent, using the semantics described in `__new()`'s argument
        list.  In contrast, `subextent.offset` is, by definition, always 0.
        """
        # This loop exists just to replace lexical recursion with manual
        # recusion using an explicit stack.  We have to do this to handle
        # files larger than about 100MB, which would otherwise exceed
        # Python's recursion limit.
        #
        # Future: we might want to add detection for infinite recursion, but
        # it's a bit tricky, so skipping for now.
        stack = [[0, [(self, offset, length)]]]
        while stack:
            idx, calls = stack[-1]
            # pyre-fixme[6]: probably a bug when list compared to int
            # pyre-fixme[58]: `<=` is not supported for operand types
            #  `Union[List[Tuple[Extent, int, Optional[int]]], int]` and `int`.
            assert idx <= len(calls)
            # pyre-fixme[6]: calls can be int!
            if idx == len(calls):
                stack.pop()
                continue
            # pyre-fixme[58]: `+` is not supported for operand types
            #  `Union[List[Tuple[Extent, int, Optional[int]]], int]` and `int`.
            stack[-1][0] += 1

            # pyre-fixme[16]: calls can be int!
            action, res = _get_trimmed_leaves(*calls[idx])
            if action is Action._YIELD:
                yield res
                continue

            assert action is Action._RECURSE
            # pyre-fixme[6]: craziness
            stack.append([0, res])

    def _gen_leaf_reprs(self):
        for _, length, leaf in self.gen_trimmed_leaves():
            if leaf.content == Extent.Kind.HOLE:
                yield "h", length
            elif leaf.content == Extent.Kind.DATA:
                yield "d", length
            else:
                raise AssertionError(leaf.content)  # pragma: no cover

    def __repr__(self) -> str:
        return "".join(
            # merge adjacent leaves of the same type
            f"{k}{sum(l for _, l in leaves)}"
            for k, leaves in itertools.groupby(self._gen_leaf_reprs(), lambda kl: kl[0])
        )

    def __copy__(self) -> "Extent":
        return self  # See the docstring

    def __deepcopy__(self, memo) -> "Extent":
        return self  # See the docstring
