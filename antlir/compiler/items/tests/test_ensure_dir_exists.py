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

from ..ensure_dir_exists import EnsureDirExistsItem, ensure_dir_exists_factory
from .common import (
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
)


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


class EnsureDirExistsItemTestCase(BaseItemTestCase):
    def test_ensure_dir_exists(self):
        for item, (expected_req, expected_prov) in zip_longest(
            ensure_dir_exists_factory(from_target="t", path="/a/b/c/d"),
            [
                ("/a/b/c", "/a/b/c/d"),
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

    def test_ensure_dir_exists_command(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dir-exists-cmd")

            ensure_items = list(
                ensure_dir_exists_factory(
                    from_target="t",
                    path="/d/a/b",
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

    def test_ensure_dir_exists_command_stat_mismatch(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("ensure-dir-exists-cmd")
            good = {
                "from_target": "t",
                "into_dir": "/",
                "basename": "z",
                "mode": "u+rx",
            }
            EnsureDirExistsItem(**good).build(subvol, DUMMY_LAYER_OPTS_BA)

            # Fail on different attributes
            with self.assertRaises(subprocess.CalledProcessError):
                EnsureDirExistsItem(**{**good, "mode": "u+rwx"}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )

            with self.assertRaises(subprocess.CalledProcessError):
                EnsureDirExistsItem(**{**good, "user_group": "77:88"}).build(
                    subvol, DUMMY_LAYER_OPTS_BA
                )
