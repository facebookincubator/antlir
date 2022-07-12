#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from types import MappingProxyType
from typing import NamedTuple, Sequence

from antlir.btrfs_diff.freeze import DoNotFreeze, freeze, frozendict


class FreezeTestCase(unittest.TestCase):
    def test_primitive(self) -> None:
        s = "abracadabra"
        self.assertIs(s, freeze(s))

    def test_memo(self) -> None:
        l = []
        memo = {}
        d = {"inner": {1: l, 2: l}}
        fd = freeze(d, _memo=memo)
        self.assertEqual({"inner": {1: (), 2: ()}}, fd)
        self.assertIsInstance(fd, frozendict)
        self.assertIsInstance(fd["inner"], frozendict)
        self.assertIs(fd["inner"][1], fd["inner"][2])
        self.assertEqual({id(d), id(d["inner"]), id(l)}, set(memo.keys()))

    def test_custom_freeze_and_memo(self) -> None:
        first = ["first"]
        test_case = self

        # Regardless of the base, we use the `freeze` method
        for base in (object, NamedTuple):

            class Foo(base):
                def freeze(self, *, _memo):
                    test_case.assertIn(id(first), _memo)
                    return "banana"

            self.assertEqual((("first",), "banana"), freeze([first, Foo()]))
            self.assertEqual("hi", freeze(first, _memo={id(first): "hi"}))

    def test_namedtuple(self) -> None:
        class UnpairedSamples(NamedTuple):
            x: Sequence[float]
            y: Sequence[float]

        self.assertEqual(
            UnpairedSamples(x=(5.0, 6.0, 7.0), y=(3.0,)),
            freeze(UnpairedSamples(x=[5.0, 6.0, 7.0], y=[3.0])),
        )

    def test_containers(self) -> None:
        f = freeze([([]), {5}, frozenset([7]), {"a": "b"}])
        self.assertEqual(((()), {5}, {7}, {"a": "b"}), f)
        self.assertEqual(
            [tuple, frozenset, frozenset, frozendict],
            [type(i) for i in f],
        )

    def test_not_implemented(self) -> None:
        with self.assertRaises(NotImplementedError):
            freeze(object())

    def test_skip_do_not_freeze(self) -> None:
        obj = DoNotFreeze()
        self.assertIs(obj, freeze(obj))

    def test_frozendict(self) -> None:
        data = {"one": "1", "two": "2"}
        fd = frozendict(data)

        self.assertIn("one", fd)
        self.assertEqual(2, len(fd))
        self.assertEqual({"one", "two"}, set(fd))
        self.assertEqual({"one", "two"}, fd.keys())
        self.assertEqual({"1", "2"}, set(fd.values()))
        self.assertEqual("1", fd.get("one"))

        self.assertNotEqual([], fd)
        self.assertEqual(data, fd)
        self.assertEqual(MappingProxyType(data.copy()), fd)
        self.assertNotEqual([], frozendict({}))

        self.assertEqual(f"frozendict({data})", repr(fd))
        self.assertEqual(1, len({fd, frozendict(data)}))


if __name__ == "__main__":
    unittest.main()
