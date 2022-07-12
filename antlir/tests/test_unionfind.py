#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.unionfind import UnionFind


class TestUnionFind(unittest.TestCase):
    def setUp(self) -> None:
        self.union_find = UnionFind()

    def test_find(self) -> None:
        """Tests implementation details and find with path compression"""
        self.union_find.add(1)
        self.assertEqual(self.union_find._parent(1), 1)

        self.union_find.union(2, 1)
        self.assertEqual(self.union_find._parent(1), 1)
        self.assertEqual(self.union_find._parent(2), 1)

        self.assertEqual(self.union_find.find(3), 3)
        self.union_find.union(3, 2)
        self.assertEqual(self.union_find._parent(3), 1)

    def test_flatten(self) -> None:
        """Tests implementation of flatten(), which should do find on all keys"""
        self.union_find.add(1)
        self.union_find.union(3, 2)
        self.union_find.union(7, 5)
        self.union_find.union(2, 1)
        self.union_find.union(4, 2)
        self.union_find.union(5, 2)
        self.union_find.union(6, 5)

        self.union_find.add(14)
        self.union_find.union(14, 12)
        self.union_find.union(15, 13)
        self.union_find.union(16, 11)
        self.union_find.union(15, 14)
        self.union_find.union(16, 17)
        self.union_find.union(13, 11)

        self.assertEqual(self.union_find._representative_dict[1], 1)
        self.assertEqual(self.union_find._representative_dict[2], 1)
        self.assertEqual(self.union_find._representative_dict[3], 2)
        self.assertEqual(self.union_find._representative_dict[4], 1)
        self.assertEqual(self.union_find._representative_dict[5], 1)
        self.assertEqual(self.union_find._representative_dict[6], 1)
        self.assertEqual(self.union_find._representative_dict[7], 5)

        self.assertEqual(self.union_find._representative_dict[11], 17)
        self.assertEqual(self.union_find._representative_dict[12], 17)
        self.assertEqual(self.union_find._representative_dict[13], 12)
        self.assertEqual(self.union_find._representative_dict[14], 12)
        self.assertEqual(self.union_find._representative_dict[15], 13)
        self.assertEqual(self.union_find._representative_dict[16], 11)
        self.assertEqual(self.union_find._representative_dict[17], 17)

        self.union_find.flatten()

        self.assertEqual(self.union_find._representative_dict[1], 1)
        self.assertEqual(self.union_find._representative_dict[2], 1)
        self.assertEqual(self.union_find._representative_dict[3], 1)
        self.assertEqual(self.union_find._representative_dict[4], 1)
        self.assertEqual(self.union_find._representative_dict[5], 1)
        self.assertEqual(self.union_find._representative_dict[6], 1)
        self.assertEqual(self.union_find._representative_dict[7], 1)

        self.assertEqual(self.union_find._representative_dict[11], 17)
        self.assertEqual(self.union_find._representative_dict[12], 17)
        self.assertEqual(self.union_find._representative_dict[13], 17)
        self.assertEqual(self.union_find._representative_dict[14], 17)
        self.assertEqual(self.union_find._representative_dict[15], 17)
        self.assertEqual(self.union_find._representative_dict[16], 17)
        self.assertEqual(self.union_find._representative_dict[17], 17)

    def test_enumerate(self) -> None:
        union_find = UnionFind()
        union_find.add(1)
        union_find.union(2, 1)
        union_find.union(3, 2)

        count = 0
        for _ in union_find:
            count += 1
        self.assertEqual(count, 3)

        keys = list(union_find)
        self.assertEqual(keys[0], 1)

    def test_iteritems(self) -> None:
        union_find = UnionFind()
        union_find.add(1)
        union_find.union(2, 1)
        union_find.union(3, 2)

        count = 0

        for key, val in union_find.items():
            count += 1
            self.assertEqual(union_find.find(key), val)
        self.assertEqual(count, 3)

    def test_persistence(self) -> None:
        """Makes sure that once two nodes are joined, they do not split"""
        union_find = UnionFind()
        union_find.add(1)
        union_find.union(1, 2)
        self.assertEqual(union_find.find(1), 2)
        self.assertEqual(union_find.find(2), 2)
        union_find.add(1)
        union_find.add(2)
        self.assertEqual(union_find.find(1), 2)
        self.assertEqual(union_find.find(2), 2)


if __name__ == "__main__":
    unittest.main()
