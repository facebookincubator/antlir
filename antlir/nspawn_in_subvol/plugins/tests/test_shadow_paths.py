#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess

from antlir.nspawn_in_subvol.plugins.shadow_paths import SHADOWED_PATHS_ROOT

from antlir.nspawn_in_subvol.tests.base import NspawnTestBase
from antlir.subvol_utils import with_temp_subvols
from antlir.tests.layer_resource import layer_resource_subvol


_SRC_SUBVOL_PAIR = (__package__, "shadows")
_SRC_SUBVOL = layer_resource_subvol(*_SRC_SUBVOL_PAIR)


class ShadowPathTestCase(NspawnTestBase):
    def _assert_original_shadow_me(self, subvol=_SRC_SUBVOL):
        # The layer is just as the source was when we're not shadowing.
        self.assertEqual("shadow me\n", subvol.path("/real/shadow_me").read_text())

    def test_ephemeral_subvol(self):
        self._assert_original_shadow_me()
        # Whether we set up shadowing by real path or via symlinks, the
        # canonical path gets shadowed.
        real = ("--shadow-path", "/real/shadow_me", "/real_i_will_shadow")
        link = ("--shadow-path", "/link/shadow_me", "/link/i_will_shadow")
        # In the 3rd case, the shadows are redundant but identical, so
        # they're tolerated -- ambiguity or aliasing would not be tolerated.
        for shadow_args in [real, link, real + link]:
            self.assertEqual(
                b"i will shadow\n",
                self._nspawn_in(
                    _SRC_SUBVOL_PAIR,
                    [*shadow_args, "--", "cat", "/real/shadow_me"],
                    stdout=subprocess.PIPE,
                ).stdout,
            )
        # Paranoia: the original is immutable anyway.
        self._assert_original_shadow_me()

    @with_temp_subvols
    def test_path_search_and_snapshot_into(self, temp_subvols):
        self._assert_original_shadow_me()
        dest_subvol = temp_subvols.caller_will_create("shadow_dest")
        with dest_subvol.maybe_create_externally():
            self._nspawn_in(
                _SRC_SUBVOL_PAIR,
                [
                    f"--snapshot-into={dest_subvol.path()}",
                    *("--shadow-path", "shadow_me", "/real_i_will_shadow"),
                    "--setenv=PATH=/real",
                    "--",
                    # Use an absolute path to the `cp` because we broke `PATH`.
                    "/bin/cp",
                    "/real/shadow_me",
                    "/out/shadow_me",
                ],
            )
        self._assert_original_shadow_me()
        self._assert_original_shadow_me(dest_subvol)
        # The source of the copy we made while shadowed was, in fact, shadowed
        self.assertEqual(
            "i will shadow\n", dest_subvol.path("/out/shadow_me").read_text()
        )

    def test_error_on_bad_search_path(self):
        with self.assertRaisesRegex(
            AssertionError, "Non-absolute PATH: MUST/BE/ABSOLUTE"
        ):
            self._nspawn_in(
                _SRC_SUBVOL_PAIR,
                [
                    "--setenv=PATH=MUST/BE/ABSOLUTE",
                    # Valid but unused because the search path is bad.
                    *(
                        "--shadow-path",
                        "/real/shadow_me",
                        "/real_i_will_shadow",
                    ),
                    "--",
                    "bad_command_never_runs",
                ],
            )

    def test_error_on_relative_path_with_slashes(self):
        with self.assertRaisesRegex(
            AssertionError, "Neither absolute nor filename: BAD/SLASH"
        ):
            self._nspawn_in(
                _SRC_SUBVOL_PAIR,
                [
                    *("--shadow-path", "BAD/SLASH", "/real_i_will_shadow"),
                    "--",
                    "bad_command_never_runs",
                ],
            )

    def test_error_on_ambiguous_source(self):
        with self.assertRaisesRegex(KeyError, "real/shadow_me was already set"):
            self._nspawn_in(
                _SRC_SUBVOL_PAIR,
                [
                    *(
                        "--shadow-path",
                        "/real/shadow_me",
                        "/real_i_will_shadow",
                    ),
                    *("--shadow-path", "/real/shadow_me", "/other_file"),
                    "--",
                    "bad_command_never_runs",
                ],
            )

    def test_error_on_aliased_dest(self):
        with self.assertRaisesRegex(
            AssertionError, "/real_i_will_shadow' shadowed > 1 destination"
        ):
            self._nspawn_in(
                _SRC_SUBVOL_PAIR,
                [
                    *(
                        "--shadow-path",
                        "/real/shadow_me",
                        "/real_i_will_shadow",
                    ),
                    *("--shadow-path", "/other_file", "/real_i_will_shadow"),
                    "--",
                    "bad_command_never_runs",
                ],
            )

    def test_error_on_unmatched_input(self):
        for dest, src in (
            # Non-existent files
            ("/NO_SUCH_FILE", "/real_i_will_shadow"),
            ("/real/shadow_me", "/NO_SUCH_FILE"),
            # Directories
            ("/other_dir", "/real_i_will_shadow"),
            ("/real/shadow_me", "/other_dir"),
        ):
            unmatched = {dest.encode(): src.encode()}
            with self.assertRaisesRegex(
                AssertionError, f"not existing, regular files: {unmatched}"
            ):
                self._nspawn_in(
                    _SRC_SUBVOL_PAIR,
                    [
                        *("--shadow-path", dest, src),
                        "--",
                        "bad_command_never_runs",
                    ],
                )

    @with_temp_subvols
    def test_copy_and_move_back(self, temp_subvols):
        self._assert_original_shadow_me()
        dest_subvol = temp_subvols.caller_will_create("shadow_copy_move")
        with dest_subvol.maybe_create_externally():
            self.assertEqual(
                b"i will shadow\n",
                self._nspawn_in(
                    _SRC_SUBVOL_PAIR,
                    [
                        f"--snapshot-into={dest_subvol.path()}",
                        *(
                            "--shadow-path",
                            "/real/shadow_me",
                            "/real_i_will_shadow",
                        ),
                        "--",
                        "sh",
                        "-uexc",
                        f"""\
                    echo UPDATED >> {(
                        SHADOWED_PATHS_ROOT / 'real/shadow_me'
                    ).shell_quote()}
                    cat /real/shadow_me  # the file is still shadowed
                    """,
                    ],
                    stdout=subprocess.PIPE,
                ).stdout,
            )
        self._assert_original_shadow_me()
        # The shadowed file got updated
        self.assertEqual(
            "shadow me\nUPDATED\n",
            dest_subvol.path("/real/shadow_me").read_text(),
        )
        # The shadow root got cleaned up
        self.assertFalse(os.path.exists(dest_subvol.path(SHADOWED_PATHS_ROOT)))

    def test_skip_unmatched_rpm(self):
        # We test shadowing a layer with the the rpm installer file removed.
        # This is to make sure that we can
        self._nspawn_in(
            (__package__, "shadows-no-rpm"),
            [
                "--user=root",
                "--",
                "echo",
                "test",
            ],
        )
