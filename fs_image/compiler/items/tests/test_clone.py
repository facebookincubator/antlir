#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import sys

from fs_image.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    require_directory,
)
from fs_image.tests.layer_resource import layer_resource_subvol
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..clone import CloneItem
from ..common import image_source_item
from .common import DUMMY_LAYER_OPTS, BaseItemTestCase, pop_path, render_subvol


_SRC_SUBVOL = layer_resource_subvol(__package__, "src-layer")


class InstallFileItemTestCase(BaseItemTestCase):
    def _clone_item(
        self,
        src,
        dest,
        *,
        omit_outer_dir=False,
        pre_existing_dest=False,
        subvol=None,
    ):
        subvol = subvol or _SRC_SUBVOL
        # The dummy object works here because `subvolumes_dir` of `None`
        # runs `artifacts_dir` internally, while our "prod" path uses the
        # already-computed value.
        return image_source_item(
            CloneItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS
        )(
            from_target="t",
            dest=dest,
            omit_outer_dir=omit_outer_dir,
            pre_existing_dest=pre_existing_dest,
            source={"layer": subvol, "path": src},
            source_layer=subvol,
        )

    def test_phase_order(self):
        self.assertIs(None, self._clone_item("/etc", "b").phase_order())

    def test_clone_nonexistent_source(self):
        ci = self._clone_item("/no_such_path", "/none_such")
        self.assertEqual("none_such", ci.dest)
        with self.assertRaises(subprocess.CalledProcessError):
            self._check_item(ci, set(), {require_directory("/")})
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            subvol = temp_subvols.create("test_clone_nonexistent_source")
            with self.assertRaises(subprocess.CalledProcessError):
                ci.build(subvol, DUMMY_LAYER_OPTS)

    def test_clone_file(self):
        ci = self._clone_item("/rpm_test/hello_world.tar", "/cloned_hello.tar")
        self.assertEqual("cloned_hello.tar", ci.dest)
        self._check_item(
            ci,
            {ProvidesFile(path="cloned_hello.tar")},
            {require_directory("/")},
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            subvol = temp_subvols.create("test_clone_file")
            ci.build(subvol, DUMMY_LAYER_OPTS)
            r = render_subvol(subvol)
            (ino,) = pop_path(r, "cloned_hello.tar")
            self.assertRegex(ino, "(File m444 d[0-9]+)")
            self.assertEqual(["(Dir)", {}], r)

    def _check_clone_bar(self, ci: CloneItem, subvol):
        ci.build(subvol, DUMMY_LAYER_OPTS)
        r = render_subvol(subvol)
        (ino,) = pop_path(r, "bar/even_more_hello_world.tar")
        self.assertRegex(ino, "(File m444 d[0-9]+)")
        self.assertEqual(
            [
                "(Dir)",
                {
                    "bar": [
                        "(Dir)",
                        {"baz": ["(Dir)", {"bar": ["(Symlink ..)"]}]},
                    ]
                },
            ],
            r,
        )

    def test_clone_omit_outer_dir(self):
        ci = self._clone_item(
            "/foo/bar", "/bar", omit_outer_dir=True, pre_existing_dest=True
        )
        self.assertEqual("bar", ci.dest)
        self._check_item(
            ci,
            {
                ProvidesDirectory(path="bar/baz"),
                ProvidesFile(path="bar/baz/bar"),
                ProvidesFile(path="bar/even_more_hello_world.tar"),
            },
            {require_directory("/bar")},
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            subvol = temp_subvols.create("test_clone_omit_outer_dir")
            subvol.run_as_root(["mkdir", subvol.path("bar")])
            self._check_clone_bar(ci, subvol)

    def test_clone_pre_existing_dest(self):
        ci = self._clone_item("/foo/bar", "/", pre_existing_dest=True)
        self.assertEqual("", ci.dest)
        self._check_item(
            ci,
            {
                ProvidesDirectory(path="bar"),
                ProvidesDirectory(path="bar/baz"),
                ProvidesFile(path="bar/baz/bar"),
                ProvidesFile(path="bar/even_more_hello_world.tar"),
            },
            {require_directory("/")},
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            subvol = temp_subvols.create("test_clone_pre_existing_dest")
            self._check_clone_bar(ci, subvol)

    def test_clone_special_files(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            src_subvol = temp_subvols.create("test_clone_special_files_src")
            dest_subvol = temp_subvols.create("test_clone_special_files_dest")

            src_subvol.run_as_root(["mkfifo", src_subvol.path("fifo")])
            src_subvol.run_as_root(
                ["mknod", src_subvol.path("null"), "c", "1", "3"]
            )

            for name in ["fifo", "null"]:
                ci = self._clone_item(name, name, subvol=src_subvol)
                self.assertEqual(name, ci.dest)
                self._check_item(
                    ci, {ProvidesFile(path=name)}, {require_directory("/")}
                )
                ci.build(dest_subvol, DUMMY_LAYER_OPTS)

            src_r = render_subvol(src_subvol)
            dest_r = render_subvol(dest_subvol)
            self.assertEqual(src_r, dest_r)
            self.assertEqual(
                ["(Dir)", {"fifo": ["(FIFO)"], "null": ["(Char 103)"]}], dest_r
            )

    def test_clone_hardlinks(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            src_subvol = temp_subvols.create("test_clone_hardlinks_src")
            dest_subvol = temp_subvols.create("test_clone_hardlinks_dest")

            src_subvol.run_as_root(["touch", src_subvol.path("a")])
            src_subvol.run_as_root(
                ["ln", src_subvol.path("a"), src_subvol.path("b")]
            )

            ci = self._clone_item(
                "/",
                "/",
                omit_outer_dir=True,
                pre_existing_dest=True,
                subvol=src_subvol,
            )
            self.assertEqual("", ci.dest)
            self._check_item(
                ci,
                {
                    ProvidesFile(path="a"),
                    ProvidesFile(path="b"),
                    # This looks like a bug (there's no /meta on disk here) but
                    # it's really just an artifact of how this path is
                    # protected.  Read: This Is Fine (TM).
                    ProvidesDoNotAccess(path="/meta"),
                },
                {require_directory("/")},
            )
            ci.build(dest_subvol, DUMMY_LAYER_OPTS)

            src_r = render_subvol(src_subvol)
            dest_r = render_subvol(dest_subvol)
            self.assertEqual(src_r, dest_r)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        # Witness that they have the same (rendered) inode # of "0"
                        "a": [["(File)", 0]],
                        "b": [["(File)", 0]],
                    },
                ],
                dest_r,
            )

    # This test makes the "special files" and "hardlinks" tests redundant,
    # because `create_ops` also covers those, but the other tests already
    # work and provide more explicit coverage.
    #
    # In terms of new coverage, this covers cloning reflinked & sparse
    # extents, as well as non-default ownership / permissions, and xattrs.
    def test_clone_demo_sendstream(self):
        src_subvol = layer_resource_subvol(__package__, "create_ops")
        ci = self._clone_item(
            "/",
            "/",
            omit_outer_dir=True,
            pre_existing_dest=True,
            subvol=src_subvol,
        )
        self.assertEqual({require_directory("/")}, set(ci.requires()))
        self.assertGreater(len(set(ci.provides())), 1)
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            dest_subvol = temp_subvols.create("create_ops")
            ci.build(dest_subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                render_subvol(src_subvol), render_subvol(dest_subvol)
            )
