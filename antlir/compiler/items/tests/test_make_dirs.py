#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    require_directory,
)
from antlir.tests.temp_subvolumes import TempSubvolumes

from ..make_dirs import MakeDirsItem
from .common import (
    DUMMY_LAYER_OPTS,
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
)


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


class MakeDirsItemTestCase(BaseItemTestCase):
    def test_make_dirs(self):
        self._check_item(
            MakeDirsItem(from_target="t", into_dir="x", path_to_make="y/z"),
            {ProvidesDirectory(path="x/y"), ProvidesDirectory(path="x/y/z")},
            {require_directory("x")},
        )

    def _test_make_dirs_command(self, layer_opts):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("tar-sv")
            subvol.run_as_root(["mkdir", subvol.path("d")])

            MakeDirsItem(
                from_target="t",
                path_to_make="/a/b/",
                into_dir="/d",
                user_group="77:88",
                mode="u+rx",
            ).build(subvol, layer_opts)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "d": [
                            "(Dir)",
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

            # The "should never happen" cases -- since we have build-time
            # checks, for simplicity/speed, our runtime clobbers permissions
            # of preexisting directories, and quietly creates non-existent
            # ones with default permissions.
            MakeDirsItem(
                from_target="t",
                path_to_make="a",
                into_dir="/no_dir",
                user_group="4:0",
            ).build(subvol, layer_opts)
            MakeDirsItem(
                from_target="t",
                path_to_make="a/new",
                into_dir="/d",
                user_group="5:0",
            ).build(subvol, layer_opts)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "d": [
                            "(Dir)",
                            {
                                # permissions overwritten for this whole tree
                                "a": [
                                    "(Dir o5:0)",
                                    {
                                        "b": ["(Dir o5:0)", {}],
                                        "new": ["(Dir o5:0)", {}],
                                    },
                                ]
                            },
                        ],
                        "no_dir": [
                            "(Dir)",
                            {"a": ["(Dir o4:0)", {}]},  # default permissions!
                        ],
                    },
                ],
                render_subvol(subvol),
            )

    def test_make_dirs_command_non_ba(self):
        self._test_make_dirs_command(DUMMY_LAYER_OPTS)

    def test_make_dirs_command_ba(self):
        self._test_make_dirs_command(DUMMY_LAYER_OPTS_BA)
