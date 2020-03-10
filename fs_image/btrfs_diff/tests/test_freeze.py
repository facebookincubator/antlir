#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from types import MappingProxyType
from typing import NamedTuple, Sequence

from ..freeze import freeze


class FreezeTestCase(unittest.TestCase):

    def test_primitive(self):
        s = 'abracadabra'
        self.assertIs(s, freeze(s))

    def test_memo(self):
        l = []
        memo = {}
        d = {'inner': {1: l, 2: l}}
        fd = freeze(d, _memo=memo)
        self.assertEqual({'inner': {1: (), 2: ()}}, fd)
        self.assertIsInstance(fd, MappingProxyType)
        self.assertIsInstance(fd['inner'], MappingProxyType)
        self.assertIs(fd['inner'][1], fd['inner'][2])
        self.assertEqual({id(d), id(d['inner']), id(l)}, set(memo.keys()))

    def test_custom_freeze_and_memo(self):
        first = ['first']
        test_case = self

        # Regardless of the base, we use the `freeze` method
        for base in (object, NamedTuple):

            class Foo(base):
                def freeze(self, *, _memo):
                    test_case.assertIn(id(first), _memo)
                    return 'banana'

            self.assertEqual((('first',), 'banana'), freeze([first, Foo()]))
            self.assertEqual('hi', freeze(first, _memo={id(first): 'hi'}))

    def test_namedtuple(self):

        class UnpairedSamples(NamedTuple):
            x: Sequence[float]
            y: Sequence[float]

        self.assertEqual(
            UnpairedSamples(x=(5., 6., 7.), y=(3.,)),
            freeze(UnpairedSamples(x=[5., 6., 7.], y=[3.])),
        )

    def test_containers(self):
        f = freeze([([]), {5}, frozenset([7]), {'a': 'b'}])
        self.assertEqual(((()), {5}, {7}, {'a': 'b'}), f)
        self.assertEqual(
            [tuple, frozenset, frozenset, MappingProxyType],
            [type(i) for i in f],
        )

    def test_not_implemented(self):
        with self.assertRaises(NotImplementedError):
            freeze(object())


if __name__ == '__main__':
    unittest.main()
