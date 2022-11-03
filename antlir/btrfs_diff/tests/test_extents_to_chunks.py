#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import itertools
import math
import re
import textwrap
from typing import Iterable, Tuple

from antlir.btrfs_diff.extent import Extent
from antlir.btrfs_diff.extents_to_chunks import extents_to_chunks_with_clones
from antlir.btrfs_diff.inode import Chunk
from antlir.btrfs_diff.inode_id import InodeID, InodeIDMap

from antlir.tests.common import AntlirTestCase


def _gen_ranges_from_figure(figure: str):
    for s in textwrap.dedent(figure.strip("\n")).split("\n"):
        s = s.rstrip()
        # Number lines should aid reading off positions. Check they're right.
        if re.match("[0-9]*$", s):
            assert ("0123456789" * math.ceil(len(s) / 10))[
                : len(s)
            ] == s, f"Bad number line {s} in {figure}"
            continue
        offset = 0
        for m in re.finditer(r"(.)\1*", s):
            v = m.group(0)
            if v[0] != " ":
                yield v[0], offset, len(v)
            offset += len(v)


def _repr_ids_and_chunks(ids_and_chunks: Iterable[Tuple[InodeID, Chunk]]):
    return {
        repr(id): [
            (
                f"{c.kind.name}/{c.length}",
                # I don't want to think about clone ordering within a chunk,
                # since it's largely arbitrary and not very important.
                {repr(cc) for cc in c.chunk_clones},
            )
            for c in chunks
        ]
        for id, chunks in ids_and_chunks
    }


class ExtentsToChunksTestCase(AntlirTestCase):
    """
    This test has one main focus, plus a few additional checks.

    Primarily: Given a "backing" DATA `Extent` whose slices are cloned into
    a few `Extent`s representing files, we must compute a correct
    presentation of those files' clone structure.

    Additionally, I exercise the case of multiple extents in a "kitchen
    sink" fashion in `test_multi_extent`.  I did not go for more
    reductionistic tests because -- with the exception of chunk merging in
    the final pass -- the code in `extents_to_chunks.py` does not allow
    interactions between different leaf extents.  Instead, it loops over
    each extent separately.  The main risk with multiple-extent scenarios is
    incorrect keying / aggregation of the clones.  This is covered amply by
    the present test, and by in-code assertions that verify we only interact
    with one extent at a time.

    Besides exercising clone detection in extents, we need to check:
     - that chunk merging works correctly (including clones), and
     - that the extent-relative coordinates of `ChunkClones` are correctly
       trimmed to match the file extent's `gen_trimmed_leaves` output.
    Both of these are covered by `test_FIG1_*` -- look at the underlying
    trimmed leaf extents in `test_gen_ids_and_extents_from_figure_*` to
    confirm this.  Then, `test_multi_extent` gives even more coverage.

    ## How to read these tests

    Most tests use `_repr_ids_and_chunks` to compactly verify outputs. Read
    this to understand its notation.

    (1) An file's data fork is a sequence of chunks. The next example refers
        to a hole (unallocated space filled with 0s) of 10 bytes, followed
        by actual data-on-disk of 20 bytes.  The two ...  sets refer to
        `clone` links to other files.  Cloning is a mechanism by which btrfs
        shares the same blocks on disk to represent identical data in
        different file locations -- in particular, this makes `cp` nearly
        instant, and allows filesystem deduplication.

        [('HOLE/10'x, {...}), ('DATA/20', {...})]

    (1) The notation for ChunkClone objects is as follows:

        <file paths>:<file offset>+<length>@<offset in the current chunk>

        So, the first clone of `('DATA/30', {'A:123+17@5', ...})` states
        that the 17 bytes of this data chunk starting from offset 5 are
        cloned (share storage with) the 123rd byte of file "A".

    ## How to read the "figure" notation

    Many tests use the "figure" notation to concisely describe how many
    different files clone their chunks from a single underlying Extent.
    Reading a figure pretty much as expected, but I will spell out a simple
    example for explicitness -- also look at `test_docstring_example`:

        AAA  AAA
          BBBBCCCCCC
         CCC
        012345678901

    The numbered bottom line serves solely to help the reader quickly
    identify lengths and offsets. The letter lines say that:
     - File A concatenates a clone of length 3 at offset 0 with a second
       clone of length 3 at offset 5.  The file's total length is thus 6.
     - File B consists of a single clone at offset 2, length 4.
     - C concatenates two clones: offset 1, length 3 with offset 6, length 6.
     - Thus, offset 2 of the portrayed Extent is cloned into all 3 files.
     - Review the ChunkClones for this figure in `test_docstring_example`.

    IMPORTANT: Figures must NOT specify two overlapping ranges for the same
    filename -- that does not make sense for `clone` operations.  The
    warning is here because `_gen_ranges_from_figure` does not detect this,
    and the downstream failure will be hard to debug.
    """

    def setUp(self) -> None:
        super().setUp()
        self.id_map = InodeIDMap.new()

    def _gen_ids_and_extents_from_figure(
        self,
        s: str,
        extent_left: int = 0,
        extent_right: int = 0,
        slice_spacing: int = 0,
    ):
        """
        Given a figure,

         - Create a backing DATA Extent, from which every file will clone.

         - Parse from the figure the sequential slices of the Extent that are
           identified by contiguous sequences of letters (aka filenames). A
           slice is an offset & length.

         - Create an `InodeID` + file `Extent` for each mentioned filename.

         - Pack, from left-to-right, each filename's slices of the backing
           Extent into the corresponding file Extent.  Optionally, we will
           separate the slices by HOLEs of `slice_spacing` bytes.  Note that
           a `slice_spacing` hole is also inserted at the beginning of each
           file.  The intent of this option is simply to check that we
           don't compute the right file offsets by accident.

         - Similarly, `extent_{left,right}` add uncloned bytes on the left &
           right of the backing extent.
        """
        ranges = sorted(_gen_ranges_from_figure(s))
        source_extent = Extent.empty().write(  # A single DATA extent
            offset=0,
            length=extent_left
            + max(offset + length for _, offset, length in ranges)
            + extent_right,
        )
        for name, group in itertools.groupby(ranges, key=lambda x: x[0]):
            file_extent = Extent.empty()
            file_offset = slice_spacing
            for _, offset, length in group:
                file_extent = file_extent.clone(
                    to_offset=file_offset,
                    from_extent=source_extent,
                    from_offset=extent_left + offset,
                    length=length,
                )
                file_offset += slice_spacing + length
            yield (
                self.id_map.add_file(self.id_map.next(), name.encode()),
                file_extent,
            )

    def _repr_chunks_from_figure(self, s: str, **kwargs):
        return _repr_ids_and_chunks(
            # pyre-fixme[6]: For 1st param expected `Iterable[Tuple[InodeID,
            #  Chunk]]` but got `Iterable[Tuple[InodeID, Sequence[Chunk]]]`.
            extents_to_chunks_with_clones(
                list(self._gen_ids_and_extents_from_figure(s, **kwargs))
            )
        )

    def test_gen_ranges_from_figure(self) -> None:
        self.assertEqual(
            [
                ("A", 0, 9),
                ("A", 16, 3),
                ("B", 9, 5),
                ("C", 5, 9),
                ("D", 3, 7),
                ("E", 10, 7),
                ("F", 11, 2),
            ],
            sorted(_gen_ranges_from_figure(self.FIG1)),
        )

    def test_gen_ranges_from_figure_empty(self) -> None:
        self.assertEqual([], list(_gen_ranges_from_figure("01234")))

    def test_gen_ranges_from_figure_bad_number_line(self) -> None:
        with self.assertRaises(AssertionError):
            list(_gen_ranges_from_figure("10234"))

    def test_gen_ids_and_extents_from_figure(self) -> None:
        # The numeric line has 21 chars, but it is not counted.
        e = Extent(Extent.Kind.DATA, 0, 19)
        self.assertEqual(
            {
                "A": [(0, 9, e), (16, 3, e)],
                "B": [(9, 5, e)],
                "C": [(5, 9, e)],
                "D": [(3, 7, e)],
                "E": [(10, 7, e)],
                "F": [(11, 2, e)],
            },
            {
                repr(id): list(extent.gen_trimmed_leaves())
                for id, extent in self._gen_ids_and_extents_from_figure(self.FIG1)
            },
        )

    def test_gen_ids_and_extents_from_figure_with_extent_left_and_right(
        self,
    ) -> None:
        # Check extent_left & extent_right
        e = Extent(Extent.Kind.DATA, 0, 2119)
        self.assertEqual(
            {
                "A": [(100, 9, e), (116, 3, e)],
                "B": [(109, 5, e)],
                "C": [(105, 9, e)],
                "D": [(103, 7, e)],
                "E": [(110, 7, e)],
                "F": [(111, 2, e)],
            },
            {
                repr(id): list(extent.gen_trimmed_leaves())
                for id, extent in self._gen_ids_and_extents_from_figure(
                    self.FIG1, extent_left=100, extent_right=2000
                )
            },
        )

    def test_gen_ids_and_extents_from_figure_with_all_kwargs(self) -> None:
        # Check slice_spacing -- the leaf trimming offsets & lengths do not
        # change, but we do get HOLEs in the expected places.
        e = Extent(Extent.Kind.DATA, 0, 2119)
        hole = (0, 17, Extent(Extent.Kind.HOLE, 0, 17))
        self.assertEqual(
            {
                "A": [hole, (100, 9, e), hole, (116, 3, e)],
                "B": [hole, (109, 5, e)],
                "C": [hole, (105, 9, e)],
                "D": [hole, (103, 7, e)],
                "E": [hole, (110, 7, e)],
                "F": [hole, (111, 2, e)],
            },
            {
                repr(id): list(extent.gen_trimmed_leaves())
                for id, extent in self._gen_ids_and_extents_from_figure(
                    self.FIG1,
                    extent_left=100,
                    extent_right=2000,
                    slice_spacing=17,
                )
            },
        )

    FIG1 = """
               FF   AAA
         CCCCCCCCC
       DDDDDDDEEEEEEE
    AAAAAAAAABBBBB
    012345678901234567890
    """

    # The result without `slice_spacing` is the same even if we vary
    # `extent_left` & `extent_right` -- that internal representation
    # detail should not be captured in our file-offset clone list.
    FIG1_repr_chunks_no_spacing = {
        # This shows successful merging of chunks -- the preceding test
        # demonstrated that we emit 2 trimmed leaves of size 9 & 3 for
        # A, but here they became one chunk of 12:
        "A": [("DATA/12", {"C:0+4@5", "D:0+6@3", "E:6+1@9"})],
        "B": [("DATA/5", {"C:4+5@0", "D:6+1@0", "E:0+4@1", "F:0+2@2"})],
        "C": [("DATA/9", {"A:5+4@0", "B:0+5@4", "D:2+5@0", "E:0+4@5", "F:0+2@6"})],
        "D": [("DATA/7", {"A:3+6@0", "B:0+1@6", "C:0+5@2"})],
        "E": [("DATA/7", {"A:9+1@6", "B:1+4@0", "C:5+4@0", "F:0+2@1"})],
        "F": [("DATA/2", {"B:2+2@0", "C:6+2@0", "E:1+2@0"})],
    }

    # The next three tests are the only tests that checks that
    # `extent_left`, `extent_right`, and `slice_spacing` do not break
    # `extents_to_chunks_with_clones`.  The single example in FIG1 is rich
    # enough that using it to exercise these offset variations gives us a
    # confidence in our position arithmetic.

    def test_finalize_FIG1(self) -> None:
        self.assertEqual(
            self.FIG1_repr_chunks_no_spacing,
            self._repr_chunks_from_figure(self.FIG1),
        )

    def test_finalize_FIG1_with_extent_left_and_right(self) -> None:
        self.assertEqual(
            self.FIG1_repr_chunks_no_spacing,
            self._repr_chunks_from_figure(
                self.FIG1, extent_left=100, extent_right=2000
            ),
        )

    def test_finalize_FIG1_with_spacing_extent_left_and_right(self) -> None:
        # Adding slice_spacing shifts all the file offsets, and adds holes
        hole = ("HOLE/100", set())
        self.assertEqual(
            {
                # This time, chunk merging is prevented by the hole.
                "A": [
                    hole,
                    ("DATA/9", {"C:100+4@5", "D:100+6@3"}),
                    hole,
                    ("DATA/3", {"E:106+1@0"}),
                ],
                "B": [
                    hole,
                    (
                        "DATA/5",
                        {"C:104+5@0", "D:106+1@0", "E:100+4@1", "F:100+2@2"},
                    ),
                ],
                "C": [
                    hole,
                    (
                        "DATA/9",
                        {
                            "A:105+4@0",
                            "B:100+5@4",
                            "D:102+5@0",
                            "E:100+4@5",
                            "F:100+2@6",
                        },
                    ),
                ],
                "D": [
                    hole,
                    ("DATA/7", {"A:103+6@0", "B:100+1@6", "C:100+5@2"}),
                ],
                "E": [
                    hole,
                    (
                        "DATA/7",
                        {"A:209+1@6", "B:101+4@0", "C:105+4@0", "F:100+2@1"},
                    ),
                ],
                "F": [
                    hole,
                    ("DATA/2", {"B:102+2@0", "C:106+2@0", "E:101+2@0"}),
                ],
            },
            self._repr_chunks_from_figure(
                self.FIG1, extent_left=17, extent_right=23, slice_spacing=100
            ),
        )

    def test_nothing_cloned(self) -> None:
        self.assertEqual(
            {"a": [("DATA/4", set())], "b": [("DATA/6", set())]},
            self._repr_chunks_from_figure("aabbbaabbb"),
        )

    def test_docstring_example(self) -> None:
        # Note that A's and C's disjoint chunks are merged before output.
        self.assertEqual(
            {
                "A": [("DATA/6", {"B:0+1@2", "B:3+1@3", "C:0+2@1", "C:3+2@4"})],
                "B": [("DATA/4", {"A:2+1@0", "A:3+1@3", "C:1+2@0"})],
                "C": [("DATA/9", {"A:1+2@0", "A:4+2@3", "B:0+2@1"})],
            },
            self._repr_chunks_from_figure(
                """
            AAA  AAA
              BBBBCCCCCC
             CCC
            012345678901
        """
            ),
        )

    def test_a_and_b_are_subsequences_of_c(self) -> None:
        self.assertEqual(
            {
                "a": [("DATA/6", {"c:0+3@0", "c:5+2@3", "c:12+1@5"})],
                "b": [("DATA/4", {"c:3+2@0", "c:7+2@2"})],
                "c": [
                    (
                        "DATA/14",
                        {
                            "a:0+3@0",
                            "a:3+2@5",
                            "a:5+1@12",
                            "b:0+2@3",
                            "b:2+2@7",
                        },
                    )
                ],
            },
            self._repr_chunks_from_figure(
                """
              cccccccccccccc
              aaabbaabb   a
            01234567890123456789
        """
            ),
        )

    def test_four_identical_clones(self) -> None:
        self.assertEqual(
            {
                "a": [("DATA/1", {"b:0+1@0", "c:0+1@0", "d:0+1@0"})],
                "b": [("DATA/1", {"a:0+1@0", "c:0+1@0", "d:0+1@0"})],
                "c": [("DATA/1", {"a:0+1@0", "b:0+1@0", "d:0+1@0"})],
                "d": [("DATA/1", {"a:0+1@0", "b:0+1@0", "c:0+1@0"})],
            },
            self._repr_chunks_from_figure("d\nc\na\nb"),
        )

    def test_ladder_of_clones(self) -> None:
        self.assertEqual(
            {
                "a": [("DATA/3", {"b:0+2@1", "c:0+1@2"})],
                "b": [("DATA/3", {"a:1+2@0", "c:0+2@1", "d:0+1@2"})],
                "c": [("DATA/3", {"a:2+1@0", "b:1+2@0", "d:0+2@1", "e:0+1@2"})],
                "d": [("DATA/3", {"b:2+1@0", "c:1+2@0", "e:0+2@1", "f:0+1@2"})],
                "e": [("DATA/3", {"c:2+1@0", "d:1+2@0", "f:0+2@1"})],
                "f": [("DATA/3", {"d:2+1@0", "e:1+2@0"})],
            },
            self._repr_chunks_from_figure(
                """
               ddd
              ccc
             bbbeee
            aaa  fff
            01234567890123456789
        """
            ),
        )

    def test_cannot_merge_adjacent_clones(self) -> None:
        # Shows that we don't currently have merging of adjacent clones
        self.assertEqual(
            {
                "a": [("DATA/4", {"b:0+2@0", "b:2+2@2"})],
                "b": [("DATA/4", {"a:0+2@0", "a:2+2@2"})],
            },
            self._repr_chunks_from_figure(
                """
            bbaa
            aabb
            01234567890123456789
        """
            ),
        )

    def test_multi_extent(self) -> None:
        # There are 3 `write` commands below, one for each of `a`, `b`, and
        # `c`.  We also create a few HOLE leaf extents along the way.  All
        # of these extents are spread around by a few clone operations, so
        # this is at true multi-extent test.
        a = Extent.empty().write(offset=3, length=5)

        b = (
            Extent.empty()
            .truncate(length=10)
            .write(offset=7, length=10)
            .clone(to_offset=5, from_extent=a, from_offset=2, length=4)
        )

        # Besides verifying `b`'s content, this gives us a way to refer to it.
        _, _, (b_trunc, a_hole, a_wr, b_wr) = zip(*b.gen_trimmed_leaves())
        self.assertEqual(b_trunc, Extent(Extent.Kind.HOLE, 0, 10))
        self.assertEqual(a_hole, Extent(Extent.Kind.HOLE, 0, 3))
        self.assertEqual(a_wr, Extent(Extent.Kind.DATA, 0, 5))
        self.assertEqual(b_wr, Extent(Extent.Kind.DATA, 0, 10))
        b_trimmed_leaves = [
            (0, 5, b_trunc),
            (2, 1, a_hole),
            (0, 3, a_wr),
            (2, 8, b_wr),
        ]
        self.assertEqual(b_trimmed_leaves, list(b.gen_trimmed_leaves()))

        c = Extent.empty().write(offset=0, length=30)
        c_wr = c  # Save this Extent for later
        self.assertEqual(c_wr, Extent(Extent.Kind.DATA, 0, 30))
        c = c.clone(to_offset=10, from_extent=a, from_offset=1, length=3).clone(
            to_offset=25, from_extent=b, from_offset=0, length=b.length
        )

        # Here is what `c` looks like on disk.
        c_trimmed_leaves = [
            (0, 10, c_wr),
            (1, 2, a_hole),
            (0, 1, a_wr),
            (13, 12, c_wr),
            *b_trimmed_leaves,
        ]
        self.assertEqual(c_trimmed_leaves, list(c.gen_trimmed_leaves()))

        a = a.clone(to_offset=4, from_extent=c, from_offset=0, length=c.length)

        # The final form of `a`
        a_trimmed_leaves = [(0, 3, a_hole), (0, 1, a_wr), *c_trimmed_leaves]
        self.assertEqual(a_trimmed_leaves, list(a.gen_trimmed_leaves()))

        # Now that we have these messy, multi-extent, self-referential
        # files, let's make sure the clone detection does the right thing.
        # Also add an empty file to make sure that corner case works.

        ids_and_chunks = list(
            extents_to_chunks_with_clones(
                [
                    (self.id_map.add_file(self.id_map.next(), p), e)
                    for p, e in [
                        (b"a", a),
                        (b"b", b),
                        (b"c", c),
                        (b"e", Extent.empty()),
                    ]
                ]
            )
        )

        expected_dict = {
            "b": [
                (
                    "HOLE/6",
                    {  # merged: `b_trunc` (5) + `a_hole` (1) = 6
                        "a:2+1@5",  # `a_hole` original location in `a`
                        "c:11+1@5",  # `a_hole` from `c.clone(10, a)`
                        "c:30+1@5",  # `a_hole` from `c.clone(25, b)`
                        "a:15+1@5",  # `a_hole` from `a.clone(4,c.clone(10,a))`
                        "a:34+1@5",  # `a_hole` from `a.clone(4,c.clone(25,b))`
                        "c:25+5@0",  # `b_trunc` from `c.clone(25, b)`
                        "a:29+5@0",  # `b_trunc` from `a.clone(4,c.clone(25,b))`
                    },
                ),
                (
                    "DATA/11",
                    {  # merged: `a_wr` (3) + `b_wr` (8) = 11
                        "a:3+1@0",  # `a_wr` original location in `a`
                        "c:12+1@0",  # `a_wr` from `c.clone(10, a)`
                        "a:16+1@0",  # `a_wr` from `a.clone(4, c.clone(10, a))`
                        "c:31+3@0",  # `a_wr` from `c.clone(25, b)`
                        "a:35+3@0",  # `a_wr` from `a.clone(4, c.clone(25, b))`
                        "c:34+8@3",  # `b_wr` from `c.clone(25, b)`
                        "a:38+8@3",  # `b_wr` from `a.clone(4, c.clone(25, b))`
                    },
                ),
            ],
            "c": [
                ("DATA/10", {"a:4+10@0"}),  # `c_wr`, via `a.clone(4, c)`
                (
                    "HOLE/2",
                    {  # `a_hole`
                        # `b` instance & its copy in `c`, and the next one in
                        # `a`
                        "b:5+1@1",
                        "c:30+1@1",
                        "a:34+1@1",
                        "a:1+2@0",  # original instance in `a`
                        "a:14+2@0",  # copy of this `c` instance into `a`
                    },
                ),
                (
                    "DATA/13",
                    {  # merged `a_wr` (1) + `c_wr` (12) = 13
                        # `a_wr`: `b` instance, copy in `c`, and it copy in `a`
                        "b:6+1@0",
                        "c:31+1@0",
                        "a:35+1@0",
                        "a:3+1@0",  # `a_wr`: original instance in `a`
                        "a:16+1@0",  # `a_wr`: copy of this `c` instance in `a`
                        "a:17+12@1",  # `c_wr` copy in `a`
                    },
                ),
                # These two are just the copy of `b` inside `c`, so this is
                # a copy-paste, replacing references into `c`'s copy of `b`
                # with references into the original `b`.
                (
                    "HOLE/6",
                    {
                        # `a` clones are unchanged
                        "a:2+1@5",
                        "a:15+1@5",
                        "a:34+1@5",
                        "a:29+5@0",
                        # `c` but unchanged, is outside of this copy of `b` in
                        # `c`
                        "c:11+1@5",
                        # In `Inode: b`, these linked to us. We return the
                        # favor.
                        "b:0+5@0",
                        "b:5+1@5",
                    },
                ),
                (
                    "DATA/11",
                    {
                        # unchanged
                        "a:3+1@0",
                        "a:16+1@0",
                        "a:35+3@0",
                        "a:38+8@3",
                        "c:12+1@0",
                        # links to this `c` chunk replaced by `b` counterparts
                        "b:6+3@0",
                        "b:9+8@3",
                    },
                ),
            ],
            "a": [
                (
                    "HOLE/3",
                    {  # `a_hole`
                        "b:5+1@2",  # `a_hole` copy from `b`
                        # These are copied with minor changes from b/17, HOLE/6
                        "c:10+2@1",  # `a_hole` from `c.clone(10, a)`
                        "c:30+1@2",  # `a_hole` from `c.clone(25, b)`
                        "a:14+2@1",  # `a_hole` from `a.clone(4,c.clone(10,a))`
                        "a:34+1@2",  # `a_hole` from `a.clone(4,c.clone(25,b))`
                    },
                ),
                (
                    "DATA/11",
                    {  # merged: `a_wr` + `c_wr` from `c` copy
                        "c:0+10@1",  # original `c_wr`
                        "b:6+1@0",  # `a_wr` copy from `b`
                        # These are copied with trivial changes from b/17,
                        # DATA/11
                        "c:12+1@0",  # `a_wr` from `c.clone(10, a)`
                        "a:16+1@0",  # `a_wr` from `a.clone(4, c.clone(10, a))`
                        "c:31+1@0",  # `a_wr` from `c.clone(25, b)`
                        "a:35+1@0",  # `a_wr` from `a.clone(4, c.clone(25, b))`
                    },
                ),
                # The rest are copy-pasted from `c`, with the appropriate
                # references to the current chunk of `a` replaced by the
                # corresponding link to `c` (subtract 4 from offset).
                (
                    "HOLE/2",
                    {  # `a_hole`
                        "b:5+1@1",
                        "c:30+1@1",
                        "a:34+1@1",
                        "a:1+2@0",  # copy-pasta
                        "c:10+2@0",  # `c` counterpart
                    },
                ),
                (
                    "DATA/13",
                    {  # merged `a_wr` (1) + `c_wr` (12) = 13
                        "b:6+1@0",
                        "c:31+1@0",
                        "a:35+1@0",
                        "a:3+1@0",  # copy-pasta
                        "c:12+1@0",
                        "c:13+12@1",  # `c` counterparts
                    },
                ),
                (
                    "HOLE/6",
                    {
                        "a:2+1@5",
                        "a:15+1@5",
                        "c:11+1@5",
                        "b:0+5@0",
                        "b:5+1@5",
                        "c:30+1@5",
                        "c:25+5@0",  # `c` counterparts (via `b` copy)
                    },
                ),
                (
                    "DATA/11",
                    {
                        "a:3+1@0",
                        "a:16+1@0",
                        "c:12+1@0",
                        "b:6+3@0",
                        "b:9+8@3",
                        "c:31+3@0",
                        "c:34+8@3",  # `c` counterparts (via `b` copy)
                    },
                ),
            ],
            "e": [],
        }
        # I iteratively built this up from the "trimmed leaves" data above,
        # and checked against the real output, one file at a time.  So, this
        # is not just "blind gold data", but a real worked example.
        # pyre-fixme[6]: For 1st param expected `Iterable[Tuple[InodeID, Chunk]]`
        #  but got `List[Tuple[InodeID, Sequence[Chunk]]]`.
        self.assertEqual(expected_dict, _repr_ids_and_chunks(ids_and_chunks))
