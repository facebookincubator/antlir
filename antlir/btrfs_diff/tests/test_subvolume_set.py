#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import dataclasses

from antlir.tests.common import AntlirTestCase

from ..freeze import freeze
from ..parse_dump import SendStreamItems
from ..rendered_tree import emit_all_traversal_ids
from ..subvolume_set import SubvolumeSet, SubvolumeSetMutator
from .subvolume_utils import expected_subvol_add_traversal_ids


class SubvolumeSetTestCase(AntlirTestCase):
    """
    This does not test applying `SendStreamItems` from `Subvolume` or
    `IncompleteInode` becasuse those classes have their own tests.
    """

    def _check_repr(self, expected, subvol_set: SubvolumeSet) -> None:
        self.assertEqual(
            *[
                {desc: emit_all_traversal_ids(sv) for desc, sv in ser.items()}
                for ser in (
                    # Subvolumes are independent, they don't share inode IDs.
                    {
                        desc: expected_subvol_add_traversal_ids(ser_subvol)
                        for desc, ser_subvol in expected.items()
                    },
                    subvol_set.map(lambda subvol: subvol.render()),
                )
            ]
        )

    def test_subvolume_set(self) -> None:
        si = SendStreamItems
        subvols = SubvolumeSet.new()
        # We'll check that freezing the SubvolumeSet at various points
        # results in an object that is not affected by future mutations.
        reprs_and_frozens = []

        # Make a tiny subvolume
        cat_mutator = SubvolumeSetMutator.new(
            subvols, si.subvol(path=b"cat", uuid=b"abe", transid=3)
        )
        cat_mutator.apply_item(si.mkfile(path=b"from"))
        # pyre-fixme[6]: For 3rd param expected `bytes` but got `str`.
        cat_mutator.apply_item(si.write(path=b"from", offset=0, data="hi"))
        cat_mutator.apply_item(si.mkfile(path=b"to"))
        cat_mutator.apply_item(si.mkfile(path=b"hole"))
        cat_mutator.apply_item(si.truncate(path=b"hole", size=5))
        bad_clone = si.clone(
            path=b"to",
            offset=0,
            from_uuid=b"BAD",
            # pyre-fixme[6]: For 4th param expected `bytes` but got `int`.
            from_transid=3,
            from_path=b"from",
            clone_offset=0,
            len=2,
        )
        with self.assertRaisesRegex(RuntimeError, "Unknown from_uuid "):
            cat_mutator.apply_item(bad_clone)
        cat_mutator.apply_item(dataclasses.replace(bad_clone, from_uuid=b"abe"))
        cat = cat_mutator.subvolume
        self.assertEqual("cat", repr(cat.id_map.inner.description))
        self.assertEqual("cat", repr(cat.id_map.inner.description))

        reprs_and_frozens.append(
            (
                {
                    "cat": [
                        "(Dir)",
                        {
                            "from": ["(File d2(cat@to:0+2@0))"],
                            "to": ["(File d2(cat@from:0+2@0))"],
                            "hole": ["(File h5)"],
                        },
                    ]
                },
                freeze(subvols),
            )
        )
        self._check_repr(*reprs_and_frozens[-1])

        # `tiger` is a snapshot of `cat`
        tiger_mutator = SubvolumeSetMutator.new(
            subvols,
            si.snapshot(
                path=b"tiger",
                uuid=b"ee",
                transid=7,
                parent_uuid=b"abe",
                parent_transid=3,  # Use the UUID of `cat`
            ),
        )
        tiger = tiger_mutator.subvolume

        self.assertIs(
            subvols.name_uuid_prefix_counts,
            tiger.id_map.inner.description.name_uuid_prefix_counts,
        )
        self.assertEqual("cat", repr(cat.id_map.inner.description))
        self.assertEqual("tiger", repr(tiger.id_map.inner.description))

        tiger_mutator.apply_item(si.unlink(path=b"from"))
        tiger_mutator.apply_item(si.unlink(path=b"hole"))
        reprs_and_frozens.append(
            (
                {
                    "cat": [
                        "(Dir)",
                        {
                            "from": ["(File d2(cat@to:0+2@0/tiger@to:0+2@0))"],
                            "to": ["(File d2(cat@from:0+2@0/tiger@to:0+2@0))"],
                            "hole": ["(File h5)"],
                        },
                    ],
                    "tiger": [
                        "(Dir)",
                        {"to": ["(File d2(cat@from:0+2@0/cat@to:0+2@0))"]},
                    ],
                },
                freeze(subvols),
            )
        )
        self._check_repr(*reprs_and_frozens[-1])

        # Check our accessors
        self.assertEqual(
            [
                "(Dir)",
                "(Dir)",
                "(File d2)",
                "(File d2)",
                "(File d2)",
                "(File h5)",
            ],
            sorted(repr(ino) for ino in subvols.inodes()),
        )
        self.assertEqual(
            {"cat": "(File h5)", "tiger": "None"},
            subvols.map(lambda sv: repr(sv.inode_at_path(b"hole"))),
        )
        self.assertEqual(
            "(File h5)",
            # pyre-fixme[16]: Optional type has no attribute `inode_at_path`.
            repr(subvols.get_by_rendered_id("cat").inode_at_path(b"hole")),
        )

        # Clone some data from `cat@hole` into `tiger@to`.
        tiger_mutator.apply_item(
            si.clone(
                path=b"to",
                offset=1,
                len=2,
                from_uuid=b"abe",
                # pyre-fixme[6]: For 5th param expected `bytes` but got `int`.
                from_transid=3,
                from_path=b"hole",
                clone_offset=2,
            )
        )
        # Note that the tiger@to references shrink to 1 bytes.
        reprs_and_frozens.append(
            (
                {
                    "cat": [
                        "(Dir)",
                        {
                            "from": ["(File d2(cat@to:0+2@0/tiger@to:0+1@0))"],
                            "to": ["(File d2(cat@from:0+2@0/tiger@to:0+1@0))"],
                            "hole": ["(File h5(tiger@to:1+2@2))"],
                        },
                    ],
                    "tiger": [
                        "(Dir)",
                        {
                            "to": [
                                "(File d1(cat@from:0+1@0/cat@to:0+1@0)"
                                "h2(cat@hole:2+2@0))"
                            ]
                        },
                    ],
                },
                freeze(subvols),
            )
        )
        self._check_repr(*reprs_and_frozens[-1])

        # Get `repr` to show some disambiguation
        cat2 = SubvolumeSetMutator.new(
            subvols, si.subvol(path=b"cat", uuid=b"app", transid=3)
        ).subvolume
        self.assertEqual("cat@ab", repr(cat.id_map.inner.description))
        self.assertEqual("cat@ap", repr(cat2.id_map.inner.description))
        reprs_and_frozens.append(
            (
                {
                    "cat@ap": ["(Dir)", {}],
                    # Difference from the previous: `s/cat/cat@ab/`
                    "cat@ab": [
                        "(Dir)",
                        {
                            "from": [
                                "(File d2(cat@ab@to:0+2@0/tiger@to:0+1@0))"
                            ],
                            "to": [
                                "(File d2(cat@ab@from:0+2@0/tiger@to:0+1@0))"
                            ],
                            "hole": ["(File h5(tiger@to:1+2@2))"],
                        },
                    ],
                    "tiger": [
                        "(Dir)",
                        {
                            "to": [
                                "(File d1(cat@ab@from:0+1@0/cat@ab@to:0+1@0)"
                                "h2(cat@ab@hole:2+2@0))"
                            ]
                        },
                    ],
                },
                freeze(subvols),
            )
        )

        # The keys of `get_by_rendered_id` follow the disambiguation.
        self.assertEqual(None, subvols.get_by_rendered_id("cat"))
        self.assertEqual(
            None, subvols.get_by_rendered_id("cat@ap").inode_at_path(b"hole")
        )
        self.assertEqual(
            "(File h5)",
            repr(subvols.get_by_rendered_id("cat@ab").inode_at_path(b"hole")),
        )

        # Now create an ambiguous repr.
        tiger2 = SubvolumeSetMutator.new(
            subvols, si.subvol(path=b"tiger", uuid=b"eep", transid=3)
        ).subvolume
        self.assertEqual("tiger@ee-ERROR", repr(tiger.id_map.inner.description))
        self.assertEqual("tiger@eep", repr(tiger2.id_map.inner.description))

        # This ensures that the frozen SubvolumeSets did not get changed
        # by mutations on the original.
        for expected, frozen in reprs_and_frozens:
            self._check_repr(expected, frozen)

    def test_errors(self) -> None:
        si = SendStreamItems
        subvols = SubvolumeSet.new()

        with self.assertRaisesRegex(RuntimeError, "must specify subvolume"):
            SubvolumeSetMutator.new(subvols, si.mkfile(path=b"foo"))

        with self.assertRaisesRegex(KeyError, "lala-uuid-foo"):
            SubvolumeSetMutator.new(
                subvols,
                si.snapshot(
                    path=b"x",
                    uuid=b"y",
                    transid=5,
                    parent_uuid=b"lala-uuid-foo",
                    parent_transid=3,
                ),
            )

        def insert_cat(transid):
            SubvolumeSetMutator.new(
                subvols, si.subvol(path=b"cat", uuid=b"a", transid=transid)
            )

        insert_cat(3)
        with self.assertRaisesRegex(RuntimeError, " is already in use: "):
            insert_cat(555)
