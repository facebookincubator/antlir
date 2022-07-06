#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from dataclasses import dataclass, field
from typing import List

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    RequireDirectory,
    RequireGroup,
    RequireUser,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes, with_temp_subvols
from antlir.tests.layer_resource import layer_resource_subvol

from ..common import (
    image_source_item,
    ImageItem,
    is_path_protected,
    make_path_normal_relative,
    META_DIR,
    META_FLAVOR_FILE,
    protected_path_set,
    setup_meta_dir,
)
from ..ensure_dirs_exist import EnsureDirsExistItem
from ..install_file import InstallFileItem
from .common import BaseItemTestCase, DUMMY_LAYER_OPTS


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

    @with_temp_subvols
    def test_image_source_item_from_layer(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols)
        setup_meta_dir(subvol, self._get_layer_opts())
        path_in_layer = "test_source_dir"
        subvol.run_as_root(["mkdir", subvol.path(path_in_layer)])
        item = image_source_item(
            FakeImageSourceItem,
            exit_stack=None,
            layer_opts=DUMMY_LAYER_OPTS,
        )(
            from_target="m",
            source={"layer": subvol, "path": path_in_layer},
            kitteh="meow",
        )
        self.assertEqual(subvol.path(path_in_layer), item.source)

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
                user="cat",
                group="dog",
            ),
            {ProvidesDirectory(path=Path("x/y/z"))},
            {
                RequireDirectory(path=Path("x/y")),
                RequireUser("cat"),
                RequireGroup("dog"),
            },
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

    @with_temp_subvols
    def test_protected_path_set_no_meta_dir(self, temp_subvols):
        subvol = temp_subvols.create("protected_path_set")
        self.assertEqual({META_DIR}, protected_path_set(subvol))

    def test_protected_path_set(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("protected_path_set")
            subvol.run_as_root(
                ["mkdir", "-p", subvol.path(".meta/private/mount/a/b/c/MOUNT")]
            )
            subvol.overwrite_path_as_root(
                Path(".meta/private/mount/a/b/c/MOUNT/is_directory"),
                contents=b"true",
            )
            subvol.run_as_root(
                ["mkdir", "-p", subvol.path(".meta/private/mount/d/e/f/MOUNT")]
            )
            subvol.overwrite_path_as_root(
                Path(".meta/private/mount/d/e/f/MOUNT/is_directory"),
                contents=b"false",
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

    def _get_layer_opts(
        self,
        build_appliance=None,
        flavor="antlir_test",
        unsafe_bypass_flavor_check=False,
    ):
        return DUMMY_LAYER_OPTS._replace(
            build_appliance=build_appliance,
            flavor=flavor,
            unsafe_bypass_flavor_check=unsafe_bypass_flavor_check,
        )

    def _setup_flavor_test_subvol(self, temp_subvolumes, flavor=None):
        subvol = temp_subvolumes.create("subvol")
        subvol.run_as_root(["mkdir", subvol.path(META_DIR)])

        if flavor:
            subvol.overwrite_path_as_root(META_FLAVOR_FILE, flavor)

        return subvol

    @with_temp_subvols
    def test_build_appliance_flavor_mismatch_error(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols)
        with self.assertRaisesRegex(AssertionError, "of the build appliance"):
            setup_meta_dir(
                subvol,
                self._get_layer_opts(
                    build_appliance=layer_resource_subvol(
                        __package__, "test-build-appliance"
                    ),
                    flavor="wrong",
                ),
            )

    @with_temp_subvols
    def test_flavor_file_exists_do_nothing(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(
            temp_subvols, flavor="antlir_test"
        )
        setup_meta_dir(subvol, self._get_layer_opts())

        self.assertEqual("antlir_test", subvol.read_path_text(META_FLAVOR_FILE))

    @with_temp_subvols
    def test_flavor_file_exists_mismatch_error(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols, flavor="wrong")
        with self.assertRaisesRegex(AssertionError, "given differs"):
            setup_meta_dir(
                subvol,
                self._get_layer_opts(),
            )

    @with_temp_subvols
    def test_write_flavor(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols)
        setup_meta_dir(
            subvol,
            self._get_layer_opts(),
        )

        self.assertEqual("antlir_test", subvol.read_path_text(META_FLAVOR_FILE))

    @with_temp_subvols
    def test_overwrite_flavor(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols, "wrong")
        setup_meta_dir(
            subvol,
            self._get_layer_opts(unsafe_bypass_flavor_check=True),
        )

        self.assertEqual("antlir_test", subvol.read_path_text(META_FLAVOR_FILE))

    @with_temp_subvols
    def test_meta_dir_already_exists(self, temp_subvols):
        subvol = self._setup_flavor_test_subvol(temp_subvols)
        setup_meta_dir(subvol, self._get_layer_opts())
        # run again to ensure META_ARTIFACTS_REQUIRE_REPO cleanup works
        setup_meta_dir(subvol, self._get_layer_opts())

    def test_phase_order(self):
        self.assertIsNone(ImageItem(from_target="t").phase_order())

    def test_make_path_normal_relative_meta_check(self):
        with self.assertRaisesRegex(AssertionError, "cannot start with .meta/"):
            make_path_normal_relative("/.meta/foo")
