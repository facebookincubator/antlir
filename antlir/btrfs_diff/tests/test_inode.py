#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import stat

from antlir.btrfs_diff.extent import Extent
from antlir.btrfs_diff.extents_to_chunks import extents_to_chunks_with_clones
from antlir.btrfs_diff.inode import (
    _repr_time,
    _repr_time_delta,
    _time_delta,
    Chunk,
    ChunkClone,
    Clone,
    Inode,
    InodeOwner,
    InodeUtimes,
)
from antlir.btrfs_diff.inode_id import InodeIDMap

from antlir.tests.common import AntlirTestCase


class InodeTestCase(AntlirTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.id_map = InodeIDMap.new()

    def _complete_inode(self, file_type, **kwargs):
        kwargs.setdefault("mode", 0o644)
        kwargs.setdefault("xattrs", {})
        return Inode(
            file_type=file_type,
            owner=InodeOwner(uid=3, gid=5),
            utimes=InodeUtimes(ctime=(2, 1), mtime=(7, 1), atime=(9, 10**6)),
            **kwargs,
        )

    def test_inode(self) -> None:
        # Shared `repr` boilerplate for the output of `self._complete_inode`.
        O = "o3:5"
        T = "t70/01/01.00:00:02+5+2"
        R = f"m644 {O} {T}"

        # Test a fully populated Inode with some clones -- this is not more
        # exhaustive because we also get lots of coverage from the `for`
        # loop below, and some more some functional coverage from
        # `test_incomplete_inode`.
        extent = Extent.empty()
        extent = extent.write(offset=15, length=5)
        extent = extent.clone(
            to_offset=0, from_extent=extent, from_offset=10, length=10
        )
        chunks_repr = "h5(me:10+5@0)d5(me:15+5@0)h5(me:0+5@0)d5(me:5+5@0)"
        my_id = self.id_map.add_file(self.id_map.next(), b"me")
        ((got_id, chunks),) = extents_to_chunks_with_clones([(my_id, extent)])
        self.assertIs(got_id, my_id)
        self.assertEqual(
            f"(File {R} x'a'='b','c'='d' {chunks_repr})",
            repr(
                self._complete_inode(
                    stat.S_IFREG, chunks=chunks, xattrs={b"a": b"b", b"c": b"d"}
                )
            ),
        )

        # Ensure the maximally "incomplete" case works.
        ino_not_complete = Inode(
            file_type=stat.S_IFREG,
            mode=None,
            owner=None,
            utimes=None,
            xattrs={},
        )
        self.assertEqual("(File)", repr(ino_not_complete))
        with self.assertRaisesRegex(
            RuntimeError, "must have file_type, owner & utimes"
        ):
            ino_not_complete.assert_valid_and_complete()

        # Trip the remaining `assert_valid_and_complete` checks, while also
        # ensuring that `repr` works in each of the cases.  Each failure is
        # preceded by a nearly-identical success.
        # pyre-fixme[6]: For 3rd param expected `Set[ChunkClone]` but got `Tuple[]`.
        chunk_d11 = Chunk(kind=Extent.Kind.DATA, length=11, chunk_clones=())
        for good, expected_repr, kwargs in [
            (1, f"(Dir {R})", {"file_type": stat.S_IFDIR, "mode": 0o644}),
            # bad mode bits
            (
                0,
                f"(Dir m40644 {O} {T})",
                {"file_type": stat.S_IFDIR, "mode": stat.S_IFDIR | 0o644},
            ),
            (1, f"(Dir {R})", {"file_type": stat.S_IFDIR}),
            (
                0,
                f"(16804 {R})",
                {"file_type": stat.S_IFDIR | 0o644},
            ),  # bad file_type bits
            (1, f"(File {R})", {"file_type": stat.S_IFREG, "chunks": ()}),
            (0, f"(File {R})", {"file_type": stat.S_IFREG}),  # lacks `.chunks`
            (1, f"(Char {R} 123)", {"file_type": stat.S_IFCHR, "dev": 0x123}),
            (0, f"(Char {R})", {"file_type": stat.S_IFCHR}),  # lacks `.dev`
            (1, f"(Block {R} 123)", {"file_type": stat.S_IFBLK, "dev": 0x123}),
            (0, f"(Block {R})", {"file_type": stat.S_IFBLK}),  # lacks `.dev`
            (
                1,
                f"(Symlink {O} {T} foo)",
                {"file_type": stat.S_IFLNK, "mode": None, "dest": b"foo"},
            ),
            (
                0,
                f"(Symlink {O} {T})",
                {"file_type": stat.S_IFLNK, "mode": None},
            ),  # lacks `.dest`
            (
                0,
                f"(Symlink {R} foo)",
                {"file_type": stat.S_IFLNK, "dest": b"foo"},
            ),  # `.mode` was set
            # Add extra fields that don't belong to the file-type.
            # The "success" cases for these are already shown above.
            (
                0,
                f"(File {R} d11 123)",
                {
                    "file_type": stat.S_IFREG,
                    "chunks": (chunk_d11,),
                    "dev": 0x123,
                },
            ),
            (
                0,
                f"(Char {R} 123 ohai)",
                {"file_type": stat.S_IFCHR, "dev": 0x123, "dest": b"ohai"},
            ),
            (
                0,
                f"(Block {R} 123 ohai)",
                {"file_type": stat.S_IFBLK, "dev": 0x123, "dest": b"ohai"},
            ),
            (
                0,
                f"(Symlink {O} {T} d11 foo)",
                {
                    "file_type": stat.S_IFLNK,
                    "mode": None,
                    "dest": b"foo",
                    "chunks": (chunk_d11,),
                },
            ),
        ]:
            with self.subTest(repr((good, expected_repr, kwargs))):
                ino = self._complete_inode(kwargs.pop("file_type"), **kwargs)
                self.assertEqual(expected_repr, repr(ino))
                if good:
                    ino.assert_valid_and_complete()
                else:
                    with self.assertRaises(RuntimeError):
                        ino.assert_valid_and_complete()

    def test_chunk_clone(self) -> None:
        clone = Clone(
            inode_id=self.id_map.add_file(self.id_map.next(), b"a"),
            offset=17,
            length=3,
        )
        self.assertEqual("a:17+3", repr(clone))
        self.assertEqual("a:17+3@22", repr(ChunkClone(offset=22, clone=clone)))

    def test_chunk(self) -> None:
        chunk = Chunk(kind=Extent.Kind.DATA, length=12, chunk_clones=set())
        self.assertEqual("(DATA/12)", repr(chunk))
        ino_id = self.id_map.add_file(self.id_map.next(), b"a")
        chunk.chunk_clones.add(
            ChunkClone(offset=3, clone=Clone(inode_id=ino_id, offset=7, length=2))
        )
        self.assertEqual("(DATA/12: a:7+2@3)", repr(chunk))
        chunk.chunk_clones.add(
            ChunkClone(offset=4, clone=Clone(inode_id=ino_id, offset=5, length=6))
        )
        self.assertIn(
            repr(chunk),  # The set can be in one of two orders
            ("(DATA/12: a:7+2@3, a:5+6@4)", "(DATA/12: a:5+6@4, a:7+2@3)"),
        )

    def test_repr_owner(self) -> None:
        self.assertEqual("12:345", repr(InodeOwner(uid=12, gid=345)))

    def test_time_delta(self) -> None:
        self.assertEqual((-1, 999999999), _time_delta((0, 0), (0, 1)))
        self.assertEqual((0, 1), _time_delta((0, 1), (0, 0)))
        self.assertEqual((-4, 999999999), _time_delta((3, 0), (6, 1)))
        self.assertEqual((3, 2), _time_delta((5, 4), (2, 2)))

    def test_repr_time_delta(self) -> None:
        self.assertEqual("-3", _repr_time_delta(-3, 0))
        self.assertEqual("-3", _repr_time_delta(-4, 999999999))
        self.assertEqual("-3.001", _repr_time_delta(-4, 999000000))
        self.assertEqual("+3", _repr_time_delta(3, 0))
        self.assertEqual("+3", _repr_time_delta(3, 1))
        self.assertEqual("+3.001", _repr_time_delta(3, 1000000))

    def test_repr_time(self) -> None:
        self.assertEqual("70/05/23.21:21:18.91", _repr_time(12345678, 910111213))

    def test_repr_utimes(self) -> None:
        self.assertEqual(
            "70/05/23.21:21:18.001+7230.01-3610.6",
            repr(
                InodeUtimes(
                    ctime=(12345678, 1000000),
                    mtime=(12345678 + 7230, 11000000),
                    atime=(12345678 + 7230 - 3611, 411000000),
                )
            ),
        )
