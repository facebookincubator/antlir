#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Tests `requires_provides.py`.
"""
import unittest

from antlir.fs_utils import Path

from ..requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
    RequireGroup,
    Requirement,
    RequirementKind,
    _normalize_path,
    require_directory,
    require_file,
)


class RequiresProvidesTestCase(unittest.TestCase):
    def test_normalize_path(self):
        self.assertEqual(Path("/a"), _normalize_path(Path("a//.")))
        self.assertEqual(Path("/b/d"), _normalize_path(Path("/b/c//../d")))
        self.assertEqual(Path("/x/y"), _normalize_path(Path("///x/./y/")))

    def test_path_normalization(self):
        self.assertEqual(Path("/a"), require_directory(Path("a//.")).path)
        self.assertEqual(
            Path("/b/d"), ProvidesDirectory(path=Path("/b/c//../d")).req.path
        )
        self.assertEqual(
            Path("/x/y"), ProvidesFile(path=Path("///x/./y/")).req.path
        )

    def test_provides_requires(self):
        pf1 = ProvidesFile(path=Path("f"))
        pf2 = ProvidesFile(path=Path("f/b"))
        pf3 = ProvidesFile(path=Path("f/b/c"))
        pd1 = ProvidesDirectory(path=Path("a"))
        pd2 = ProvidesDirectory(path=Path("a/b"))
        pd3 = ProvidesDirectory(path=Path("a/b/c"))
        provides = [pf1, pf2, pf3, pd1, pd2, pd3]

        rf1 = require_file(Path("f"))
        rf2 = require_file(Path("f/b"))
        rf3 = require_file(Path("f/b/c"))
        rd1 = require_directory(Path("a"))
        rd2 = require_directory(Path("a/b"))
        rd3 = require_directory(Path("a/b/c"))
        requires = [rf1, rf2, rf3, rd1, rd2, rd3]

        for p in provides:
            for r in requires:
                self.assertEqual(
                    p.req.path == r.path,
                    p.provides(r),
                    f"{p}.provides({r})",
                )

    def test_provides_do_not_access(self):
        self.assertFalse(
            ProvidesDoNotAccess(path=Path("//a/b")).provides(
                require_file(Path("/a/b"))
            )
        )

    def test_with_new_path(self):
        for new_path in ["b", "b/", "/b", "/../a/../b/c/.."]:
            self.assertEqual(
                ProvidesDirectory(path=Path("unused")).with_new_path(
                    Path(new_path)
                ),
                ProvidesDirectory(path=Path("b")),
            )

    def test_requirement_key(self):
        with self.assertRaises(NotImplementedError):
            Requirement(kind=None).key()

    def test_path_requires_predicate_key(self):
        p = Path("/a/b/c")
        self.assertEqual(p, require_directory(p).key())
        self.assertEqual(p, require_file(p).key())

    def test_provides_path_object_path(self):
        p = Path("/a/b/c")
        self.assertEqual(p, ProvidesDirectory(p).path())
        self.assertEqual(p, ProvidesDirectory(p).path())

    def test_require_group(self):
        groupname = "foo"
        g = RequireGroup(groupname)
        self.assertEqual(g.name, groupname)
        self.assertEqual(g.kind, RequirementKind.GROUP)
        g2 = RequireGroup(groupname)
        self.assertEqual(1, len({g.key(), g2.key()}))

    def test_provides_group(self):
        groupname = "foo"
        pg = ProvidesGroup(groupname)
        self.assertEqual(pg.req.name, groupname)
        self.assertEqual(pg.req.kind, RequirementKind.GROUP)
        self.assertTrue(pg.provides(RequireGroup(groupname)))
