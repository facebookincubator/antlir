#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Tests `requires_provides.py`.
'''
import unittest

from ..requires_provides import (
    ProvidesDirectory, ProvidesFile, ProvidesDoNotAccess,
    require_directory, require_file, _normalize_path,
)


class RequiresProvidesTestCase(unittest.TestCase):

    def test_normalize_path(self):
        self.assertEqual('/a', _normalize_path('a//.'))
        self.assertEqual('/b/d', _normalize_path('/b/c//../d'))
        self.assertEqual('/x/y', _normalize_path('///x/./y/'))

    def test_path_normalization(self):
        self.assertEqual('/a', require_directory('a//.').path)
        self.assertEqual('/b/d', ProvidesDirectory(path='/b/c//../d').path)
        self.assertEqual('/x/y', ProvidesFile(path='///x/./y/').path)

    def test_provides_requires(self):
        pf1 = ProvidesFile(path='f')
        pf2 = ProvidesFile(path='f/b')
        pf3 = ProvidesFile(path='f/b/c')
        pd1 = ProvidesDirectory(path='a')
        pd2 = ProvidesDirectory(path='a/b')
        pd3 = ProvidesDirectory(path='a/b/c')
        provides = [pf1, pf2, pf3, pd1, pd2, pd3]

        rf1 = require_file('f')
        rf2 = require_file('f/b')
        rf3 = require_file('f/b/c')
        rd1 = require_directory('a')
        rd2 = require_directory('a/b')
        rd3 = require_directory('a/b/c')
        requires = [rf1, rf2, rf3, rd1, rd2, rd3]

        # Only these will match, everything else cannot.
        provides_matches_requires = {
            (pf1, rf1),
            (pf2, rf2),
            (pf3, rf3),
            (pd1, rd1),
            (pd1, rd2),
            (pd1, rd3),
            (pd2, rd2),
            (pd2, rd3),
            (pd3, rd3),
        }

        # TODO: Use ValidateReqsProvs here once that's committed and tested?
        path_to_reqs_provs = {}
        for p_or_r in (*provides, *requires):
            path_to_reqs_provs.setdefault(p_or_r.path, []).append(p_or_r)

        for p in provides:
            for r in requires:
                # It is an error to match Provides/Requires with distinct paths
                if p.path == r.path:
                    self.assertEqual(
                        (p, r) in provides_matches_requires,
                        p.matches(path_to_reqs_provs, r),
                        f'{p}.match({r})'
                    )
                else:
                    with self.assertRaisesRegex(
                        AssertionError, '^Tried to match .* against .*$'
                    ):
                        p.matches(path_to_reqs_provs, r)

    def test_provides_do_not_access(self):
        with self.assertRaisesRegex(
            AssertionError, '^predicate .* not implemented by .*$'
        ):
            ProvidesDoNotAccess(path='//a/b').matches(
                    {}, require_file('/a/b'))

if __name__ == '__main__':
    unittest.main()
