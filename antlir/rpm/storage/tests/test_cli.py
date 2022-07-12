#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from io import BytesIO

from antlir.fs_utils import Path, temp_dir

from antlir.rpm.storage import cli


class StorageCliTestCase(unittest.TestCase):
    def test_cli(self) -> None:
        with temp_dir() as td:
            p = b"Hello, world!"  # Write ~1.67 chunks of this phrase
            f_in = BytesIO(p * int(cli._CHUNK_SIZE * 5 / (3 * len(p))))
            f_sid = BytesIO()
            cli.main(
                [
                    "--storage",
                    Path.json_dumps(
                        {
                            "kind": "filesystem",
                            "key": "test",
                            "base_dir": td / "storage",
                        }
                    ),
                    "put",
                ],
                from_file=f_in,
                to_file=f_sid,
            )

            self.assertTrue(f_sid.getvalue().endswith(b"\n"))
            sid = f_sid.getvalue()[:-1]

            f_out = BytesIO()
            cli.main(
                [
                    "--storage",
                    Path.json_dumps(
                        {
                            "kind": "filesystem",
                            "key": "test",
                            "base_dir": td / "storage",
                        }
                    ),
                    "get",
                    sid,
                ],
                # pyre-fixme[6]: For 2nd param expected `BytesIO` but got `None`.
                from_file=None,
                to_file=f_out,
            )

            self.assertEqual(f_in.getvalue(), f_out.getvalue())
