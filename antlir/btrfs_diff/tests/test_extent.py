#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import functools
import itertools
import math
import sys
from types import SimpleNamespace

from antlir.btrfs_diff.extent import Extent

from antlir.tests.common import AntlirTestCase


class ExtentTestCase(AntlirTestCase):
    def test_write_into_empty(self) -> None:
        # Writing at offset 0 does not create a hole.
        self.assertEqual(
            Extent(Extent.Kind.DATA, 0, 3),
            Extent.empty().write(offset=0, length=3),
        )
        # Writing at offset 5 creates a hole.
        self.assertEqual(
            Extent(
                (
                    Extent(Extent.Kind.HOLE, 0, 5),
                    Extent(Extent.Kind.DATA, 0, 6),
                ),
                0,
                11,
            ),
            Extent.empty().write(offset=5, length=6),
        )

    # When `gen_trimmed_leaves` was recursive, this would fail.
    def test_deep_recursion(self) -> None:
        n = sys.getrecursionlimit()
        # Stitch together hole-data `n` times.
        e = Extent.empty()
        for i in range(n):
            e = e.write(offset=2 * i + 1, length=1)
        self.assertEqual("h1d1" * n, repr(e))

    def test_write_and_clone(self) -> None:
        # 3-byte hole, 4-byte data, 5-byte hole, 6-byte data
        four = (
            Extent.empty().write(offset=3, length=4).write(offset=12, length=6)
        )
        self.assertEqual("h3d4h5d6", repr(four))
        _, _, (a, b, c, d) = zip(*four.gen_trimmed_leaves())
        # The "Control run" below will check that we preserved object identity.
        self.assertEqual(
            [
                Extent(Extent.Kind.HOLE, 0, 3),
                Extent(Extent.Kind.DATA, 0, 4),
                Extent(Extent.Kind.HOLE, 0, 5),
                Extent(Extent.Kind.DATA, 0, 6),
            ],
            [a, b, c, d],
        )

        # 7-byte data
        one = Extent.empty().write(offset=0, length=7)
        self.assertEqual([(0, 7, one)], list(one.gen_trimmed_leaves()))

        # An extent-to-clone with a little nesting -- but its content
        # should not matter for the result, it just affects the leaves.
        clone = (
            Extent.empty().write(offset=7, length=5).write(offset=5, length=4)
        )
        self.assertEqual("h5d7", repr(clone))  # repr merged two DATA leaves
        _, _, (ca, cb, cc) = zip(*clone.gen_trimmed_leaves())
        self.assertEqual(
            [
                Extent(Extent.Kind.HOLE, 0, 7),
                Extent(Extent.Kind.DATA, 0, 4),
                Extent(Extent.Kind.DATA, 0, 5),
            ],
            [ca, cb, cc],
        )
        clone_leaves = [(0, 5, ca), (0, 4, cb), (2, 3, cc)]  # checked below

        # Try a bunch of writes; check the result and its `gen_trimmed_leaves`
        for ns in [
            SimpleNamespace(
                msg="Control run on `four`: no writes",
                into=four,
                action=lambda e: e,
                result=four,
                # We will use offset=0 & length=e.length for `a` and `c`.
                leaves=[a, (0, 4, b), c, (0, 6, d)],
            ),
            # `four` writes at the start of the extent
            SimpleNamespace(
                msg="Write into `four` at offset 0, partly replacing `a`",
                into=four,
                action=lambda e: e.write(offset=0, length=2),
                result=Extent(
                    (Extent(Extent.Kind.DATA, 0, 2), Extent((four,), 2, 16)),
                    0,
                    18,
                ),
                leaves=[Extent(Extent.Kind.DATA, 0, 2), (2, 1, a), b, c, d],
            ),
            SimpleNamespace(
                msg="Write into `four` at offset 0, replacing all of `a`",
                into=four,
                action=lambda e: e.write(offset=0, length=3),
                result=Extent(
                    (Extent(Extent.Kind.DATA, 0, 3), Extent((four,), 3, 15)),
                    0,
                    18,
                ),
                leaves=[Extent(Extent.Kind.DATA, 0, 3), b, c, d],
            ),
            SimpleNamespace(
                msg="Write into `four` at offset 0, over `a` and some of `b`",
                into=four,
                action=lambda e: e.write(offset=0, length=5),
                result=Extent(
                    (Extent(Extent.Kind.DATA, 0, 5), Extent((four,), 5, 13)),
                    0,
                    18,
                ),
                leaves=[Extent(Extent.Kind.DATA, 0, 5), (2, 2, b), c, d],
            ),
            SimpleNamespace(
                msg="Write into `four` at offset 0, leaving just part of `d`",
                into=four,
                action=lambda e: e.write(offset=0, length=15),
                result=Extent(
                    (Extent(Extent.Kind.DATA, 0, 15), Extent((four,), 15, 3)),
                    0,
                    18,
                ),
                leaves=[Extent(Extent.Kind.DATA, 0, 15), (3, 3, d)],
            ),
            SimpleNamespace(
                msg="Write into `four` at offset 0, replacing all of `four`",
                into=four,
                action=lambda e: e.write(offset=0, length=18),
                result=Extent(Extent.Kind.DATA, 0, 18),
                leaves=[Extent(Extent.Kind.DATA, 0, 18)],
            ),
            SimpleNamespace(
                msg="Write into `four` at offset 0, go 10 bytes past its end",
                into=four,
                action=lambda e: e.write(offset=0, length=28),
                result=Extent(Extent.Kind.DATA, 0, 28),
                leaves=[Extent(Extent.Kind.DATA, 0, 28)],
            ),
            # `four` writes in the middle of the extent
            SimpleNamespace(
                msg="Write 7 bytes into `four` at offset 5",
                into=four,
                action=lambda e: e.write(offset=5, length=7),
                result=Extent(
                    (
                        Extent((four,), 0, 5),
                        Extent(Extent.Kind.DATA, 0, 7),
                        Extent((four,), 12, 6),
                    ),
                    0,
                    18,
                ),
                leaves=[a, (0, 2, b), Extent(Extent.Kind.DATA, 0, 7), d],
            ),
            SimpleNamespace(
                msg="Write 7 bytes into `four` at offset 7",
                into=four,
                action=lambda e: e.write(offset=7, length=7),
                result=Extent(
                    (
                        Extent((four,), 0, 7),
                        Extent(Extent.Kind.DATA, 0, 7),
                        Extent((four,), 14, 4),
                    ),
                    0,
                    18,
                ),
                leaves=[a, b, Extent(Extent.Kind.DATA, 0, 7), (2, 4, d)],
            ),
            # `four` write at the end of the extent
            SimpleNamespace(
                msg="Write 10 bytes into `four` at offset 8",
                into=four,
                action=lambda e: e.write(offset=8, length=10),
                result=Extent(
                    (Extent((four,), 0, 8), Extent(Extent.Kind.DATA, 0, 10)),
                    0,
                    18,
                ),
                leaves=[a, b, (0, 1, c), Extent(Extent.Kind.DATA, 0, 10)],
            ),
            # `four` write crossing the end of the extent
            SimpleNamespace(
                msg="Write 10 bytes into `four` at offset 12",
                into=four,
                action=lambda e: e.write(offset=12, length=10),
                result=Extent(
                    (Extent((four,), 0, 12), Extent(Extent.Kind.DATA, 0, 10)),
                    0,
                    22,
                ),
                leaves=[a, b, c, Extent(Extent.Kind.DATA, 0, 10)],
            ),
            # `four` writes past the end of the extent
            SimpleNamespace(
                msg="Write 2 bytes into `four` at offset 18",
                into=four,
                action=lambda e: e.write(offset=18, length=2),
                result=Extent((four, Extent(Extent.Kind.DATA, 0, 2)), 0, 20),
                leaves=[a, b, c, d, Extent(Extent.Kind.DATA, 0, 2)],
            ),
            SimpleNamespace(
                msg="Write 1 bytes into `four` at offset 19",
                into=four,
                action=lambda e: e.write(offset=19, length=1),
                result=Extent(
                    (
                        four,
                        Extent(Extent.Kind.HOLE, 0, 1),
                        Extent(Extent.Kind.DATA, 0, 1),
                    ),
                    0,
                    20,
                ),
                leaves=[
                    a,
                    b,
                    c,
                    d,
                    Extent(Extent.Kind.HOLE, 0, 1),
                    Extent(Extent.Kind.DATA, 0, 1),
                ],
            ),
            # While comprehensive, the `four` tests above are not
            # combinatorially complete (there are too many possible
            # boundaries to try to cover).
            #
            # In contrast, the intent of the `one` tests below is to
            # actually cover all the meaningful possibilities of writing on
            # top of a single extent.
            SimpleNamespace(
                msg="Control run on `one`: no writes",
                into=one,
                action=lambda e: e,
                result=one,
                leaves=[one],
            ),
            SimpleNamespace(
                msg="Write 2 bytes into `one` at offset 0",
                into=one,
                action=lambda e: e.write(offset=0, length=2),
                result=Extent(
                    (Extent(Extent.Kind.DATA, 0, 2), Extent((one,), 2, 5)), 0, 7
                ),
                leaves=[Extent(Extent.Kind.DATA, 0, 2), (2, 5, one)],
            ),
            SimpleNamespace(
                msg="Write 7 bytes into `one` at offset 0",
                into=one,
                action=lambda e: e.write(offset=0, length=7),
                result=Extent(Extent.Kind.DATA, 0, 7),
                leaves=[Extent(Extent.Kind.DATA, 0, 7)],
            ),
            SimpleNamespace(
                msg="Write 3 bytes into `one` at offset 2",
                into=one,
                action=lambda e: e.write(offset=2, length=3),
                result=Extent(
                    (
                        Extent((one,), 0, 2),
                        Extent(Extent.Kind.DATA, 0, 3),
                        Extent((one,), 5, 2),
                    ),
                    0,
                    7,
                ),
                leaves=[
                    (0, 2, one),
                    Extent(Extent.Kind.DATA, 0, 3),
                    (5, 2, one),
                ],
            ),
            SimpleNamespace(
                msg="Write 4 bytes into `one` at offset 3",
                into=one,
                action=lambda e: e.write(offset=3, length=4),
                result=Extent(
                    (Extent((one,), 0, 3), Extent(Extent.Kind.DATA, 0, 4)), 0, 7
                ),
                leaves=[(0, 3, one), Extent(Extent.Kind.DATA, 0, 4)],
            ),
            SimpleNamespace(
                msg="Write 4 bytes into `one` at offset 5",
                into=one,
                action=lambda e: e.write(offset=5, length=4),
                result=Extent(
                    (Extent((one,), 0, 5), Extent(Extent.Kind.DATA, 0, 4)), 0, 9
                ),
                leaves=[(0, 5, one), Extent(Extent.Kind.DATA, 0, 4)],
            ),
            SimpleNamespace(
                msg="Write 3 bytes into `one` at offset 7",
                into=one,
                action=lambda e: e.write(offset=7, length=3),
                result=Extent((one, Extent(Extent.Kind.DATA, 0, 3)), 0, 10),
                leaves=[one, Extent(Extent.Kind.DATA, 0, 3)],
            ),
            SimpleNamespace(
                msg="Write 2 bytes into `one` at offset 11",
                into=one,
                action=lambda e: e.write(offset=11, length=2),
                result=Extent(
                    (
                        one,
                        Extent(Extent.Kind.HOLE, 0, 4),
                        Extent(Extent.Kind.DATA, 0, 2),
                    ),
                    0,
                    13,
                ),
                leaves=[
                    one,
                    Extent(Extent.Kind.HOLE, 0, 4),
                    Extent(Extent.Kind.DATA, 0, 2),
                ],
            ),
            # We don't have to test `clone` exhaustively, since it shares
            # all of its offset-handling logic with `write`, and only
            # differs in the extent it inserts.
            SimpleNamespace(
                msg="Control run on `clone` -- checks object identity is kept",
                into=clone,
                action=lambda e: e,
                result=clone,
                leaves=clone_leaves,
            ),
            SimpleNamespace(
                msg="`clone` over `one` at offset 0",
                into=one,
                action=lambda e: e.clone(
                    to_offset=0,
                    from_extent=clone,
                    from_offset=0,
                    length=clone.length,
                ),
                result=clone,
                leaves=clone_leaves,
            ),
            SimpleNamespace(
                msg="`clone` over `one` at offset 3",
                into=one,
                action=lambda e: e.clone(
                    to_offset=3,
                    from_extent=clone,
                    from_offset=0,
                    length=clone.length,
                ),
                result=Extent(
                    (Extent((one,), 0, 3), clone), 0, clone.length + 3
                ),
                leaves=[(0, 3, one), *clone_leaves],
            ),
            SimpleNamespace(
                msg="trimmed `clone` into the middle of `one` at offset 2",
                into=one,
                action=lambda e: e.clone(
                    to_offset=2, from_extent=clone, from_offset=2, length=4
                ),
                result=Extent(
                    (
                        Extent((one,), 0, 2),
                        Extent((clone,), 2, 4),
                        Extent((one,), 6, 1),
                    ),
                    0,
                    7,
                ),
                leaves=[(0, 2, one), (2, 3, ca), (0, 1, cb), (6, 1, one)],
            ),
            SimpleNamespace(
                msg="trimmed `clone` into `four` at offset 5",
                into=four,
                action=lambda e: e.clone(
                    to_offset=5, from_extent=clone, from_offset=3, length=7
                ),
                result=Extent(
                    (
                        Extent((four,), 0, 5),
                        Extent((clone,), 3, 7),
                        Extent((four,), 12, 6),
                    ),
                    0,
                    18,
                ),
                # `cc` starts at offset 2 because `clone` already trimmed it
                leaves=[a, (0, 2, b), (3, 2, ca), cb, (2, 1, cc), d],
            ),
        ]:
            # pyre-fixme[16]: `SimpleNamespace` has no attribute `msg`.
            with self.subTest(ns.msg):
                # pyre-fixme[16]: `SimpleNamespace` has no attribute `into`.
                _, _, orig_leaves = zip(*ns.into.gen_trimmed_leaves())
                # pyre-fixme[16]: `SimpleNamespace` has no attribute `action`.
                result = ns.action(ns.into)
                # pyre-fixme[16]: `SimpleNamespace` has no attribute `result`.
                self.assertEqual(ns.result, result)
                for expected_leaf, leaf in itertools.zip_longest(
                    # pyre-fixme[16]: `SimpleNamespace` has no attribute `leaves`.
                    ns.leaves,
                    result.gen_trimmed_leaves(),
                ):
                    expected_leaf = (
                        (0, expected_leaf.length, expected_leaf)
                        if isinstance(expected_leaf, Extent)
                        else expected_leaf
                    )
                    self.assertEqual(expected_leaf, leaf)
                    # Make sure we preserve object identity, not just equality
                    if any(expected_leaf[2] is e for e in orig_leaves):
                        self.assertIs(expected_leaf[2], leaf[2])

    # `test_leaf_commutativity` also tests `truncate`.
    def test_truncate(self) -> None:
        e = Extent.empty().write(offset=3, length=7)
        for i in range(1, 10):
            self.assertEqual(Extent((e,), 0, i), e.truncate(length=i))
        self.assertEqual(
            Extent((e, Extent(Extent.Kind.HOLE, 0, 1)), 0, 11),
            e.truncate(length=11),
        )

    # A cute demonstration that while different orders of operations produce
    # different nestings, `gen_trimmed_leaves` restores commutativity.
    #
    # While it does not replace `test_write_and_clone`, the main benefit of
    # this test is scalability: it gives us a cheap-to-implement, but
    # meaningful assertion, which we can validate on a huge number of
    # sequences of operations with no manual effort.
    #
    # Additionally, it contributes "complex" case coverage for `truncate`.
    def test_leaf_commutativity(self) -> None:
        clone = (
            Extent.empty().write(offset=0, length=2).write(offset=3, length=1)
        )
        self.assertEqual("d2h1d1", repr(clone))
        # If we treat as identical holes of different provenance but the
        # same length, these operations should commute since they write data
        # to nonoverlapping regions.
        ops = [
            lambda e: e.write(offset=3, length=5),
            lambda e: e.truncate(length=25),
            lambda e: e.write(offset=1, length=1),
            lambda e: e.write(offset=17, length=2),
            lambda e: e.clone(
                to_offset=10,
                from_extent=clone,
                from_offset=0,
                length=clone.length,
            ),
        ]

        def compose_ops(ops_it):
            return functools.reduce(
                lambda extent, op: op(extent), ops_it, Extent.empty()
            )

        # All permutations make distinct nestings with the same leaf structure
        all_extents = {compose_ops(p) for p in itertools.permutations(ops)}
        self.assertEqual(math.factorial(len(ops)), len(all_extents))
        self.assertEqual(
            {
                (
                    1,
                    (
                        0,
                        1,
                        Extent(content=Extent.Kind.DATA, offset=0, length=1),
                    ),
                    1,
                    (
                        0,
                        5,
                        Extent(content=Extent.Kind.DATA, offset=0, length=5),
                    ),
                    2,
                    (
                        0,
                        2,
                        Extent(content=Extent.Kind.DATA, offset=0, length=2),
                    ),
                    1,
                    (
                        0,
                        1,
                        Extent(content=Extent.Kind.DATA, offset=0, length=1),
                    ),
                    3,
                    (
                        0,
                        2,
                        Extent(content=Extent.Kind.DATA, offset=0, length=2),
                    ),
                    6,
                )
            },
            {
                tuple(
                    # Different permutations produce the same-length holes
                    # differently, so let's only compare lengths.
                    l if se.content == Extent.Kind.HOLE else (o, l, se)
                    for o, l, se in e.gen_trimmed_leaves()
                )
                for e in all_extents
            },
        )

    # test_write_and_clone covers leaf identity, but it's still nice to
    # explicitly check that the whole nested object is cloned.
    def test_clone_preserves_identity(self) -> None:
        clone = Extent.empty().truncate(length=5)
        self.assertEqual("h5", repr(clone))
        result = (
            Extent.empty()
            .write(offset=0, length=30)
            .clone(to_offset=10, from_extent=clone, from_offset=1, length=3)
        )
        self.assertIs(clone, result.content[1].content[0])

    def test_repr_kind(self) -> None:
        self.assertEqual("Extent.Kind.DATA", repr(Extent.Kind.DATA))

    def test_empty(self) -> None:
        self.assertEqual("", repr(Extent.empty()))
        self.assertEqual([], list(Extent.empty().gen_trimmed_leaves()))

    def test_copy(self) -> None:
        e = Extent.empty().write(offset=5, length=5)
        self.assertIs(e, copy.deepcopy(e))
        self.assertIs(e, copy.copy(e))
