#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import tempfile
import unittest

from ..stat_options import mode_to_octal_str


class StatOptionsTestCase(unittest.TestCase):
    def test_mode_to_octal_str(self):
        inputs = [
            # Regular permissions 'rwx'
            "",
            "+x",
            "u+x",
            "a+rx,u+w",
            "u+w,a+xr",
            "a+wrx",
            "u+wrx,g+xrw,o+rwx",
            "u+wrx,og+xrw",
            "u+wrx,og+r",
            "uog+w",
            "u+wrx,g+xrw,o+rwx,a+r",
            "a+r,o+w",
            "a+r,a+w,u+x",
            # Sticky bit 't'
            "+t",
            "+tx",
            "u+t,a+r",
            "a+t,a+r",
            # Set on execution bit 's'
            "u+sr",
            "g+sw",
            "ug+s",
            "a+srx",
            "u+s,g+s",
            "u+s,g+s,a+s",
            "ug+s,a+trx",
        ]
        with tempfile.TemporaryDirectory() as td:
            for val in inputs:
                subprocess.check_call(
                    ["chmod", f"a-rwxXst{',' + val if val else ''}", td]
                )
                stat_res = subprocess.check_output(
                    ["stat", "--format=%a", td], text=True
                ).strip()
                stat_oct = f"{int(stat_res, base=8):04o}"
                conversion = mode_to_octal_str(val)
                self.assertEqual(
                    stat_oct,
                    conversion,
                    f"Expected {val} to produce {stat_oct}, got {conversion}",
                )

    def test_mode_to_octal_str_errs(self):
        with self.assertRaisesRegex(AssertionError, "Only append actions"):
            mode_to_octal_str("u-wx")
        with self.assertRaisesRegex(AssertionError, "Only append actions"):
            mode_to_octal_str("u=wx")
        with self.assertRaisesRegex(AssertionError, "Only classes of"):
            mode_to_octal_str("j+wx")
        with self.assertRaisesRegex(AssertionError, "Only permissions of"):
            mode_to_octal_str("a+rwk")
