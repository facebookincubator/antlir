#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile
import unittest.mock

from antlir.compiler.items.common import PhaseOrder, protected_path_set
from antlir.compiler.items.ensure_dirs_exist import ensure_subdirs_exist_factory
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.symlink import SymlinkToDirItem
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
    with_mocked_temp_volume_dir,
)

from antlir.errors import UserError
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol, TempSubvolumes

DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba(
    Subvol("test-build-appliance", already_exists=True)
)


class RemovePathItemTestCase(BaseItemTestCase):
    @with_mocked_temp_volume_dir
    def test_remove_item(self) -> None:
        with TempSubvolumes() as temp_subvolumes, tempfile.NamedTemporaryFile() as empty_tf:  # noqa: E501
            subvol = temp_subvolumes.create("remove_action")
            self.assertEqual(["(Dir)", {}], render_subvol(subvol))

            for item in reversed(
                list(
                    ensure_subdirs_exist_factory(
                        from_target="t", into_dir="/", subdirs_to_create="a/b/c"
                    )
                )
            ):
                item.build(subvol, DUMMY_LAYER_OPTS_BA)
            for d in ["d", "e"]:
                InstallFileItem(
                    from_target="t", source=empty_tf.name, dest=f"/a/b/c/{d}"
                ).build(subvol, DUMMY_LAYER_OPTS_BA)
            for item in reversed(
                list(
                    ensure_subdirs_exist_factory(
                        from_target="t", into_dir="/", subdirs_to_create="f/g"
                    )
                )
            ):
                item.build(subvol, DUMMY_LAYER_OPTS_BA)
            # Checks that `rm` won't follow symlinks
            SymlinkToDirItem(from_target="t", source="/f", dest="/a/b/f_sym").build(
                subvol, DUMMY_LAYER_OPTS_BA
            )
            for d in ["h", "i"]:
                InstallFileItem(
                    from_target="t", source=empty_tf.name, dest=f"/f/{d}"
                ).build(subvol, DUMMY_LAYER_OPTS_BA)
            SymlinkToDirItem(from_target="t", source="/f/i", dest="/f/i_sym").build(
                subvol, DUMMY_LAYER_OPTS_BA
            )
            intact_subvol = [
                "(Dir)",
                {
                    "a": [
                        "(Dir)",
                        {
                            "b": [
                                "(Dir)",
                                {
                                    "c": [
                                        "(Dir)",
                                        {
                                            "d": ["(File m444)"],
                                            "e": ["(File m444)"],
                                        },
                                    ],
                                    "f_sym": ["(Symlink ../../f)"],
                                },
                            ]
                        },
                    ],
                    "f": [
                        "(Dir)",
                        {
                            "g": ["(Dir)", {}],
                            "h": ["(File m444)"],
                            "i": ["(File m444)"],
                            "i_sym": ["(Symlink i)"],
                        },
                    ],
                },
            ]
            self.assertEqual(intact_subvol, render_subvol(subvol))

            # We refuse to touch protected paths, even with must_exist=False.
            # If the paths started with '.meta', they would trip the check in
            # `_make_path_normal_relative`, so we mock-protect 'xyz'.
            for prot_path in ["xyz", "xyz/potato/carrot"]:
                with unittest.mock.patch(
                    "antlir.compiler.items.remove_path.protected_path_set",
                    side_effect=lambda sv: protected_path_set(sv) | {Path("xyz")},
                ), self.assertRaisesRegex(
                    UserError,
                    f".*Path to be removed \\({prot_path}\\) is protected.*",
                ):
                    RemovePathItem.get_phase_builder(
                        [
                            RemovePathItem(
                                from_target="t",
                                must_exist=False,
                                path=prot_path,
                            )
                        ],
                        DUMMY_LAYER_OPTS_BA,
                    )(subvol)

            # Check handling of non-existent paths without removing anything
            remove = RemovePathItem(
                from_target="t",
                must_exist=False,
                path="/does/not/exist",
            )
            self.assertEqual(PhaseOrder.REMOVE_PATHS, remove.phase_order())
            RemovePathItem.get_phase_builder([remove], DUMMY_LAYER_OPTS_BA)(subvol)
            with self.assertRaisesRegex(UserError, "does not exist"):
                RemovePathItem.get_phase_builder(
                    [
                        RemovePathItem(
                            from_target="t",
                            must_exist=True,
                            path="/does/not/exist",
                        )
                    ],
                    DUMMY_LAYER_OPTS_BA,
                )(subvol)
            self.assertEqual(intact_subvol, render_subvol(subvol))

            # Now remove most of the subvolume.
            RemovePathItem.get_phase_builder(
                [
                    # These 3 removes are not covered by a recursive remove.
                    # And we leave behind /f/i, which lets us know that neither
                    # `f_sym` nor `i_sym` were followed during their deletion.
                    RemovePathItem(
                        from_target="t",
                        must_exist=True,
                        path="/f/i_sym",
                    ),
                    RemovePathItem(
                        from_target="t",
                        must_exist=True,
                        path="/f/h",
                    ),
                    RemovePathItem(
                        from_target="t",
                        must_exist=True,
                        path="/f/g",
                    ),
                    # The next 3 items are intentionally sequenced so that if
                    # they were applied in the given order, they would fail.
                    RemovePathItem(
                        from_target="t",
                        must_exist=False,
                        path="/a/b/c/e",
                    ),
                    RemovePathItem(
                        from_target="t",
                        must_exist=True,
                        # The surrounding items don't delete /a/b/c/d, e.g. so
                        # this recursive remove is still tested.
                        path="/a/b/",
                    ),
                    RemovePathItem(
                        from_target="t",
                        must_exist=True,
                        path="/a/b/c/e",
                    ),
                ],
                DUMMY_LAYER_OPTS_BA,
            )(subvol)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "a": ["(Dir)", {}],
                        "f": ["(Dir)", {"i": ["(File m444)"]}],
                    },
                ],
                render_subvol(subvol),
            )

    def test_remove_path_item_sort_order(self) -> None:
        self.assertEqual(
            [
                RemovePathItem(path="a", must_exist=False),
                RemovePathItem(path="a", must_exist=True),
                RemovePathItem(path="a/b", must_exist=True),
                RemovePathItem(path="a/b/c", must_exist=True),
            ],
            sorted(
                [
                    RemovePathItem(path="a/b", must_exist=True),
                    RemovePathItem(path="a/b/c", must_exist=True),
                    RemovePathItem(path="a", must_exist=True),
                    RemovePathItem(path="a", must_exist=False),
                ],
            ),
        )
