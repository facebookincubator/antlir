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

from ..common import ImageItem, image_source_item
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
            {ProvidesDirectory(path="x/y/z")},
            {require_directory("x/y")},
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
