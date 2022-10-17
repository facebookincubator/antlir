#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.compiler.items.clone import CloneItem
from antlir.compiler.items.common import image_source_item
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
    pop_path,
    render_subvol,
)

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesSymlink,
    RequireDirectory,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource_subvol


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
        subvol = subvol or layer_resource_subvol(__package__, "src-layer")
        # The dummy object works here because `subvolumes_dir` of `None`
        # runs `artifacts_dir` internally, while our "prod" path uses the
        # already-computed value.
        return image_source_item(CloneItem, layer_opts=DUMMY_LAYER_OPTS)(
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
        self.assertEqual(Path("none_such"), ci.dest)
        with self.assertRaises(subprocess.CalledProcessError):
            self._check_item(ci, set(), {RequireDirectory(path=Path("/"))})

        with TempSubvolumes() as temp_subvols:
            subvol = temp_subvols.create("test_clone_nonexistent_source")
            with self.assertRaises(subprocess.CalledProcessError):
                ci.build(subvol, DUMMY_LAYER_OPTS)

    def test_clone_file(self):
        ci = self._clone_item("/rpm_test/hello_world.tar", "/cloned_hello.tar")
        self.assertEqual(Path("cloned_hello.tar"), ci.dest)
        self._check_item(
            ci,
            {ProvidesFile(path=Path("cloned_hello.tar"))},
            {RequireDirectory(path=Path("/"))},
        )
        with TempSubvolumes() as temp_subvols:
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
        self.assertEqual(Path("bar"), ci.dest)
        self._check_item(
            ci,
            {
                ProvidesDirectory(path=Path("bar/baz")),
                ProvidesFile(path=Path("bar/even_more_hello_world.tar")),
                ProvidesSymlink(path=Path("bar/baz/bar"), target=Path("..")),
            },
            {RequireDirectory(path=Path("/bar"))},
        )
        with TempSubvolumes() as temp_subvols:
            subvol = temp_subvols.create("test_clone_omit_outer_dir")
            subvol.run_as_root(["mkdir", subvol.path("bar")])
            self._check_clone_bar(ci, subvol)

    def test_clone_pre_existing_dest(self):
        ci = self._clone_item("/foo/bar", "/", pre_existing_dest=True)
        self.assertEqual(Path(""), ci.dest)
        self._check_item(
            ci,
            {
                ProvidesDirectory(path=Path("bar")),
                ProvidesDirectory(path=Path("bar/baz")),
                ProvidesFile(path=Path("bar/even_more_hello_world.tar")),
                ProvidesSymlink(path=Path("bar/baz/bar"), target=Path("..")),
            },
            {RequireDirectory(path=Path("/"))},
        )
        with TempSubvolumes() as temp_subvols:
            subvol = temp_subvols.create("test_clone_pre_existing_dest")
            self._check_clone_bar(ci, subvol)

    def test_clone_special_files(self):
        with TempSubvolumes() as temp_subvols:
            src_subvol = temp_subvols.create("test_clone_special_files_src")
            dest_subvol = temp_subvols.create("test_clone_special_files_dest")

            src_subvol.run_as_root(["mkfifo", src_subvol.path("fifo")])
            src_subvol.run_as_root(
                ["mknod", src_subvol.path("null"), "c", "1", "3"]
            )

            for name in ["fifo", "null"]:
                ci = self._clone_item(name, name, subvol=src_subvol)
                self.assertEqual(Path(name), ci.dest)
                self._check_item(
                    ci,
                    {ProvidesFile(path=Path(name))},
                    {RequireDirectory(path=Path("/"))},
                )
                ci.build(dest_subvol, DUMMY_LAYER_OPTS)

            src_r = render_subvol(src_subvol)
            dest_r = render_subvol(dest_subvol)
            self.assertEqual(src_r, dest_r)
            self.assertEqual(
                ["(Dir)", {"fifo": ["(FIFO)"], "null": ["(Char 103)"]}], dest_r
            )

    def test_clone_hardlinks(self):
        with TempSubvolumes() as temp_subvols:
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
            self.assertEqual(Path(""), ci.dest)
            self._check_item(
                ci,
                {
                    ProvidesFile(path=Path("a")),
                    ProvidesFile(path=Path("b")),
                    # This looks like a bug (there's no /.meta on disk here) but
                    # it's really just an artifact of how this path is
                    # protected.  Read: This Is Fine (TM).
                    ProvidesDoNotAccess(path=Path("/.meta")),
                },
                {RequireDirectory(path=Path("/"))},
            )
            ci.build(dest_subvol, DUMMY_LAYER_OPTS)

            src_r = render_subvol(src_subvol)
            dest_r = render_subvol(dest_subvol)
            self.assertEqual(src_r, dest_r)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        # Witness that they have the same (rendered) inode # of
                        # "0"
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
        self.assertEqual({RequireDirectory(path=Path("/"))}, set(ci.requires()))
        self.assertGreater(len(set(ci.provides())), 1)
        with TempSubvolumes() as temp_subvols:
            dest_subvol = temp_subvols.create("volume")
            ci.build(dest_subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                render_subvol(src_subvol), render_subvol(dest_subvol)
            )
