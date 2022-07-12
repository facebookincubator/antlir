#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from itertools import zip_longest

from antlir.compiler.items.ensure_dirs_exist import (
    ensure_subdirs_exist_factory,
    EnsureDirsExistItem,
    MismatchError,
)
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
    with_mocked_temp_volume_dir,
)

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    RequireDirectory,
    RequireGroup,
    RequireUser,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol, TempSubvolumes
from pydantic import ValidationError

DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba(
    Subvol("test-build-appliance", already_exists=True)
)


class EnsureDirsExistItemTestCase(BaseItemTestCase):
    def test_ensure_subdirs_exist(self):
        for item, (expected_req, expected_prov) in zip_longest(
            ensure_subdirs_exist_factory(
                from_target="t", into_dir="/", subdirs_to_create="/a/b/c"
            ),
            [
                ("/a/b", "/a/b/c"),
                ("/a", "/a/b"),
                ("/", "/a"),
            ],
        ):
            self._check_item(
                item,
                {ProvidesDirectory(path=Path(expected_prov))},
                {
                    RequireDirectory(path=Path(expected_req)),
                    RequireUser("root"),
                    RequireGroup("root"),
                },
            )
        for item, (expected_req, expected_prov) in zip_longest(
            ensure_subdirs_exist_factory(
                from_target="t", into_dir="/w/x", subdirs_to_create="y/z"
            ),
            [
                ("/w/x/y", "/w/x/y/z"),
                ("/w/x", "/w/x/y"),
            ],
        ):
            self._check_item(
                item,
                {ProvidesDirectory(path=Path(expected_prov))},
                {
                    RequireDirectory(path=Path(expected_req)),
                    RequireUser("root"),
                    RequireGroup("root"),
                },
            )

    def test_ensure_subdirs_exist_invalid_into_dir(self):
        with self.assertRaisesRegex(ValueError, "empty string"):
            list(
                ensure_subdirs_exist_factory(
                    from_target="t", into_dir="", subdirs_to_create="/a/b"
                )
            )

    @with_mocked_temp_volume_dir
    def test_ensure_subdirs_exist_command(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-subdirs-exist-cmd")
            ensure_items = list(
                ensure_subdirs_exist_factory(
                    from_target="t",
                    into_dir="/",
                    subdirs_to_create="/d/a/b",
                    user="77",
                    group="88",
                    mode=0o500,
                )
            )
            for item in reversed(ensure_items):
                item.build(subvol, DUMMY_LAYER_OPTS_BA)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "d": [
                            "(Dir m500 o77:88)",
                            {
                                "a": [
                                    "(Dir m500 o77:88)",
                                    {"b": ["(Dir m500 o77:88)", {}]},
                                ]
                            },
                        ]
                    },
                ],
                render_subvol(subvol),
            )

    @with_mocked_temp_volume_dir
    def test_ensure_dirs_exist_item_stat_check(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dirs-exist-item")
            subvol.run_as_root(["mkdir", "-p", subvol.path("m")])
            good = {
                "from_target": "t",
                "into_dir": "m",
                "basename": "n",
                "mode": 0o600,
            }
            EnsureDirsExistItem(**good).build(subvol, DUMMY_LAYER_OPTS_BA)
            EnsureDirsExistItem(**{**good, "mode": 0o600}).build(
                subvol, DUMMY_LAYER_OPTS_BA
            )
            # Fail on different attributes
            with self.assertRaises(MismatchError):
                EnsureDirsExistItem(**{**good, "mode": 0o775}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )
            with self.assertRaises(MismatchError):
                EnsureDirsExistItem(**{**good, "mode": 0o700}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )
            with self.assertRaises(MismatchError):
                EnsureDirsExistItem(
                    **{**good, "user": "77", "group": "88"}
                ).build(subvol, DUMMY_LAYER_OPTS_BA)

    @with_mocked_temp_volume_dir
    def test_ensure_dirs_exist_item_xattrs_check(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dirs-exist-item")
            subvol.run_as_root(["mkdir", "-p", subvol.path("alpha")])
            subvol.run_as_root(["chmod", "755", subvol.path("alpha")])
            ede_item = EnsureDirsExistItem(
                from_target="t",
                into_dir="/",
                basename="alpha",
                mode=0o755,
            )
            ede_item.build(subvol, DUMMY_LAYER_OPTS_BA)
            subvol.run_as_root(
                [
                    "setfattr",
                    "-n",
                    "user.test_attr",
                    "-v",
                    "uhoh",
                    subvol.path("/alpha"),
                ]
            )
            with self.assertRaises(MismatchError):
                ede_item.build(subvol, DUMMY_LAYER_OPTS_BA)

    @with_mocked_temp_volume_dir
    def test_ensure_other_files_and_dirs_are_kept_intact(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dirs-exist-item")
            subvol.run_as_root(["mkdir", "-p", subvol.path("foo")])
            subvol.run_as_root(["chmod", "755", subvol.path("foo")])
            subvol.run_as_root(["mkdir", "-p", subvol.path("foo/inn")])
            subvol.run_as_root(["chmod", "555", subvol.path("foo/inn")])
            subvol.run_as_root(["touch", subvol.path("foo/exo")])
            subvol.run_as_root(["chmod", "4755", subvol.path("foo/exo")])
            ensure_items = list(
                ensure_subdirs_exist_factory(
                    from_target="t",
                    into_dir="/",
                    subdirs_to_create="/foo/bar",
                )
            )
            for item in reversed(ensure_items):
                item.build(subvol, DUMMY_LAYER_OPTS_BA)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "foo": [
                            "(Dir)",
                            {
                                "bar": ["(Dir)", {}],
                                "exo": ["(File m4755)"],
                                "inn": ["(Dir m555)", {}],
                            },
                        ]
                    },
                ],
                render_subvol(subvol),
            )

    def test_ensure_dirs_exist_item_disallows_subdirs_to_create(self):
        with self.assertRaises(ValidationError):
            EnsureDirsExistItem(
                from_target="t",
                into_dir="a/b",
                basename="c",
                subdirs_to_create="b/c",
            )

        # don't even allow None
        with self.assertRaises(ValidationError):
            EnsureDirsExistItem(
                from_target="t",
                into_dir="a/b",
                basename="c",
                subdirs_to_create=None,
            )
