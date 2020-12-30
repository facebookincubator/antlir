#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import sys
from itertools import zip_longest

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    require_directory,
)
from antlir.tests.temp_subvolumes import TempSubvolumes

from ..ensure_dirs_exist import (
    EnsureDirsExistItem,
    ensure_subdirs_exist_factory,
)
from .common import (
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
)


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


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
                {ProvidesDirectory(path=expected_prov)},
                {require_directory(expected_req)},
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
                {ProvidesDirectory(path=expected_prov)},
                {require_directory(expected_req)},
            )

    def test_ensure_subdirs_exist_invalid_into_dir(self):
        with self.assertRaisesRegex(ValueError, "empty string"):
            list(
                ensure_subdirs_exist_factory(
                    from_target="t", into_dir="", subdirs_to_create="/a/b"
                )
            )

    def test_ensure_subdirs_exist_command(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-subdirs-exist-cmd")
            ensure_items = list(
                ensure_subdirs_exist_factory(
                    from_target="t",
                    into_dir="/",
                    subdirs_to_create="/d/a/b",
                    user_group="77:88",
                    mode="u+rx",
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

    def test_ensure_dirs_exist_item_stat_mismatch(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dirs-exist-item")
            subvol.run_as_root(["mkdir", "-p", subvol.path("j")])
            good = {
                "from_target": "t",
                "into_dir": "j",
                "basename": "k",
                "mode": 0o777,
            }
            EnsureDirsExistItem(**good).build(subvol, DUMMY_LAYER_OPTS_BA)
            # Fail on different attributes
            with self.assertRaises(subprocess.CalledProcessError):
                EnsureDirsExistItem(**{**good, "mode": 0o775}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )
            with self.assertRaises(subprocess.CalledProcessError):
                EnsureDirsExistItem(**{**good, "user_group": "77:88"}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )

    def test_ensure_dirs_exist_item_stat_chmod_str_mismatch(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dirs-exist-item")
            subvol.run_as_root(["mkdir", "-p", subvol.path("m")])
            good = {
                "from_target": "t",
                "into_dir": "m",
                "basename": "n",
                "mode": "u+rw",
            }
            EnsureDirsExistItem(**good).build(subvol, DUMMY_LAYER_OPTS_BA)
            with self.assertRaises(subprocess.CalledProcessError):
                EnsureDirsExistItem(**{**good, "mode": "u+rwx"}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )
