#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
from unittest import mock

from antlir.fs_utils import temp_dir
from antlir.nspawn_in_subvol.run_test import (
    add_container_not_part_of_build_step,
    do_not_rewrite_cmd,
    forward_env_vars,
    rewrite_testpilot_python_cmd,
    rewrite_tpx_gtest_cmd,
)


class NspawnTestInSubvolTestCase(unittest.TestCase):
    def test_forward_env_vars(self) -> None:
        self.assertEqual([], list(forward_env_vars({"a": "b"})))
        self.assertEqual(
            ["--setenv=TEST_PILOT=xyz"],
            list(forward_env_vars({"a": "b", "TEST_PILOT": "xyz"})),
        )
        self.assertEqual(
            ["--setenv=ANTLIR_DEBUG=1", "--setenv=TEST_PILOT=foo"],
            list(
                forward_env_vars(
                    {"a": "b", "ANTLIR_DEBUG": "1", "TEST_PILOT": "foo"}
                )
            ),
        )

    def test_do_not_rewrite_cmd(self) -> None:
        # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
        with do_not_rewrite_cmd(["a", "b"], 3) as cmd_and_fds:
            self.assertEqual((["a", "b"], []), cmd_and_fds)

    def test_rewrite_testpilot_python_cmd(self) -> None:
        bin = "/layer-test-binary"

        # Test no-op rewriting
        cmd = [bin, "foo", "--bar", "beep", "--baz", "-xack", "7", "9"]
        # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
        with rewrite_testpilot_python_cmd(cmd, next_fd=1337) as cmd_and_fd:
            self.assertEqual((cmd, []), cmd_and_fd)

        for rewritten_opt in ("--output", "--list-tests"):
            with temp_dir() as td:
                tmp = td / "foo.json"
                self.assertFalse(os.path.exists(tmp))  # Will be created
                prefix = ["--zap=3", "--ou", "boo", "--ou=3"]
                suffix = ["garr", "-abc", "-gh", "-d", '--e"f']
                with rewrite_testpilot_python_cmd(
                    [bin, *prefix, f"{rewritten_opt}={tmp}", *suffix],
                    next_fd=37,
                ) as (new_cmd, fds_to_forward):
                    (fd_to_forward,) = fds_to_forward
                    self.assertIsInstance(fd_to_forward, int)
                    # The last argument deliberately requires shell quoting.
                    self.assertEqual(
                        [
                            "/bin/bash",
                            "-c",
                            " ".join(
                                [
                                    "exec",
                                    bin,
                                    rewritten_opt,
                                    ">(cat >&37)",
                                    *prefix,
                                    *suffix[:-1],
                                    """'--e"f'""",
                                ]
                            ),
                        ],
                        new_cmd,
                    )
                    self.assertTrue(os.path.exists(tmp))  # Was created

    def test_rewrite_tpx_gtest_cmd(self) -> None:
        bin = "/layer-test-binary"
        # The last argument deliberately requires shell quoting.
        cmd = [bin, "foo", "--bar", "beep", "--baz", '--e"f']

        # Test no-op rewriting
        with mock.patch.dict(
            os.environ,
            {"NO_GTEST": "env is set"}
            # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
        ), rewrite_tpx_gtest_cmd(cmd, next_fd=1337) as cmd_and_fd:
            self.assertEqual((cmd, []), cmd_and_fd)

        with temp_dir() as td:
            tmp = td / "bar.xml"
            self.assertFalse(os.path.exists(tmp))  # Will be created
            with mock.patch.dict(
                os.environ, {"GTEST_OUTPUT": f"xml:{tmp}"}
            ), rewrite_tpx_gtest_cmd(cmd, next_fd=37) as (new_cmd, fds):
                (fd_to_forward,) = fds
                self.assertIsInstance(fd_to_forward, int)
                self.assertEqual(
                    [
                        "/bin/bash",
                        "-c",
                        " ".join(
                            [
                                "GTEST_OUTPUT=xml:>(cat >&37)",
                                "exec",
                                *cmd[:-1],
                                """'--e"f'""",  # Yes, it's shell-quoted
                            ]
                        ),
                    ],
                    new_cmd,
                )
                self.assertTrue(os.path.exists(tmp))  # Was created

    def test_add_container_not_part_of_build_step(self):
        with add_container_not_part_of_build_step(["a", "b"]) as args:
            magic_flag, a, b = args
            self.assertEqual(("a", "b"), (a, b), args)
            self.assertRegex(
                magic_flag,
                "^--container-not-part-of-build-step=.*nis_domainname$",
            )
