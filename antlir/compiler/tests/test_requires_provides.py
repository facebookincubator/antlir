#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Tests `requires_provides.py`.
"""
import unittest

from antlir.fs_utils import Path

from ..requires_provides import (
    _normalize_path,
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
    ProvidesSymlink,
    ProvidesUser,
    RequireDirectory,
    RequireFile,
    RequireGroup,
    RequirementKind,
    RequireSymlink,
    RequireUser,
)


class RequiresProvidesTestCase(unittest.TestCase):
    def test_normalize_path(self):
        self.assertEqual(Path("/a"), _normalize_path(Path("a//.")))
        self.assertEqual(Path("/b/d"), _normalize_path(Path("/b/c//../d")))
        self.assertEqual(Path("/x/y"), _normalize_path(Path("///x/./y/")))

    def test_path_normalization(self):
        self.assertEqual(Path("/a"), RequireDirectory(path=Path("a//.")).path)
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

        rf1 = RequireFile(path=Path("f"))
        rf2 = RequireFile(path=Path("f/b"))
        rf3 = RequireFile(path=Path("f/b/c"))
        rd1 = RequireDirectory(path=Path("a"))
        rd2 = RequireDirectory(path=Path("a/b"))
        rd3 = RequireDirectory(path=Path("a/b/c"))
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
                RequireFile(path=Path("/a/b"))
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

    def test_provides_path_object_path(self):
        p = Path("/a/b/c")
        self.assertEqual(p, ProvidesDirectory(p).path())
        self.assertEqual(p, ProvidesDirectory(p).path())

    def test_require_group(self):
        groupname = "foo"
        g = RequireGroup(groupname)
        self.assertEqual(g.name, groupname)
        self.assertEqual(g.kind, RequirementKind.GROUP)

    def test_provides_group(self):
        groupname = "foo"
        pg = ProvidesGroup(groupname)
        self.assertEqual(pg.req.name, groupname)
        self.assertEqual(pg.req.kind, RequirementKind.GROUP)
        self.assertTrue(pg.provides(RequireGroup(groupname)))

    def test_require_user(self):
        username = "user"
        ru = RequireUser(username)
        self.assertEqual(ru.name, username)
        self.assertEqual(ru.kind, RequirementKind.USER)
        ru2 = RequireUser(username)
        self.assertEqual(ru, ru2)

    def test_provides_user(self):
        username = "user"
        pu = ProvidesUser(username)
        self.assertEqual(pu.req.name, username)
        self.assertEqual(pu.req.kind, RequirementKind.USER)
        self.assertTrue(pu.provides(RequireUser(username)))
        self.assertFalse(pu.provides(RequireUser("user2")))

    def test_require_symlink(self):
        path = Path("/foo")
        target = Path("/bar")
        rs = RequireSymlink(path=path, target=target)
        self.assertEqual(rs.kind, RequirementKind.PATH)
        self.assertEqual(rs.path, path)
        self.assertEqual(rs.target, target)

    def test_provides_symlink(self):
        path = Path("/foo")
        target = Path("/bar")
        ps = ProvidesSymlink(path=path, target=target)
        rs = RequireSymlink(path=path, target=target)
        self.assertEqual(ps.req, rs)
        self.assertTrue(ps.provides(rs))

        # Symlinks and files/dirs are different now
        self.assertFalse(ps.provides(RequireFile(path)))
        self.assertFalse(ps.provides(RequireDirectory(path)))

        new_path = Path("/baz")
        ps2 = ps.with_new_path(new_path)
        rs2 = RequireSymlink(path=new_path, target=target)
        self.assertEqual(ps2.req, rs2)
        self.assertFalse(ps2.provides(rs))
        self.assertTrue(ps2.provides(rs2))
