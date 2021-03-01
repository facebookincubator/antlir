#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from dataclasses import dataclass, field
from typing import List

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    require_directory,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes

from ..common import (
    META_DIR,
    ImageItem,
    image_source_item,
    protected_path_set,
    is_path_protected,
)
from ..ensure_dirs_exist import EnsureDirsExistItem
from ..install_file import InstallFileItem
from .common import DUMMY_LAYER_OPTS, BaseItemTestCase


@dataclass(init=False, frozen=True)
class FakeImageSourceItem(ImageItem):
    source: str
    kitteh: str
    myint: int = 1
    mylist: List = field(default_factory=list)


class ItemsCommonTestCase(BaseItemTestCase):
    def test_image_source_item(self):
        # Cover the `source=None` branch in `image_source_item`.
        it = image_source_item(
            FakeImageSourceItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS
        )(from_target="m", source=None, kitteh="meow")
        self.assertEqual(
            FakeImageSourceItem(from_target="m", source=None, kitteh="meow"), it
        )
        self.assertIsNone(it.source)
        self.assertEqual("meow", it.kitteh)

    def test_enforce_no_parent_dir(self):
        with self.assertRaisesRegex(AssertionError, r"cannot start with \.\."):
            InstallFileItem(
                from_target="t", source="/etc/passwd", dest="a/../../b"
            )

    def test_stat_options(self):
        self._check_item(
            EnsureDirsExistItem(
                from_target="t",
                into_dir="x/y",
                basename="z",
                mode=0o733,
                user_group="cat:dog",
            ),
            {ProvidesDirectory(path=Path("x/y/z"))},
            {require_directory(Path("x/y"))},
        )

    def test_image_non_default_after_default(self):
        @dataclass(init=False, frozen=True)
        class TestImageSourceItem(FakeImageSourceItem):
            invalid: str

        with self.assertRaisesRegex(TypeError, "follows default"):
            TestImageSourceItem(
                from_target="m", source="x", kitteh="y", invalid="z"
            )

    def test_image_defaults(self):
        item = FakeImageSourceItem(from_target="m", source="x", kitteh="y")
        self.assertEqual(item.myint, 1)
        self.assertEqual(item.mylist, [])

    def test_image_missing(self):
        with self.assertRaisesRegex(TypeError, "missing .* required"):
            FakeImageSourceItem(from_target="m", source="x")

    def test_image_unexpected(self):
        with self.assertRaisesRegex(TypeError, "unexpected keyword argument"):
            FakeImageSourceItem(
                from_target="m",
                source="x",
                kitteh="y",
                unexpected="a",
                another="b",
                lastone="c",
            )

    def test_protected_path_set_no_subvol(self):
        self.assertEqual({META_DIR}, protected_path_set(None))

    def test_protected_path_set(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("protected_path_set")
            subvol.run_as_root(
                ["mkdir", "-p", subvol.path(".meta/private/mount/a/b/c/MOUNT")]
            )
            subvol.run_as_root(
                [
                    "tee",
                    subvol.path(".meta/private/mount/a/b/c/MOUNT/is_directory"),
                ],
                input=b"true",
            )
            subvol.run_as_root(
                ["mkdir", "-p", subvol.path(".meta/private/mount/d/e/f/MOUNT")]
            )
            subvol.run_as_root(
                [
                    "tee",
                    subvol.path(".meta/private/mount/d/e/f/MOUNT/is_directory"),
                ],
                input=b"false",
            )
            self.assertEqual(
                {META_DIR, Path("a/b/c/"), Path("d/e/f")},
                protected_path_set(subvol),
            )

    def test_is_path_protected(self):
        for path, protected_paths, want in (
            (Path("a"), {Path("a")}, True),
            (Path("a"), {Path("b"), Path("a")}, True),
            (Path("c"), {Path("b"), Path("a")}, False),
            (Path("a/b"), {Path("a")}, True),
            (Path("a"), {Path("ab")}, False),
            (Path("ab"), {Path("a")}, False),
            (Path("a/b"), {Path("ab")}, False),
            (Path("/path/to/file/oops"), {Path("/path/to/file")}, True),
        ):
            self.assertEqual(
                want,
                is_path_protected(path, protected_paths),
                f"{path}, {protected_paths}",
            )
