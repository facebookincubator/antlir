#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from ..stat_options import mode_to_octal_str


class StatOptionsTestCase(unittest.TestCase):
    def test_mode_to_octal_str(self):
        str_to_octal = {
            "a+rx,u+w": "0755",
            "u+w,a+xr": "0755",
            "a+wrx": "0777",
            "u+wrx,g+xrw,o+rwx": "0777",
            "u+wrx,og+xrw": "0777",
            "u+wrx,og+r": "0744",
            "uog+w": "0222",
            "u+wrx,g+xrw,o+rwx,a+r": "0777",
            "a+r,o+w": "0446",
            "a+r,a+w,u+x": "0766",
            "u+x": "0100",
            "": "0000",
        }
        for val, exp in str_to_octal.items():
            res = mode_to_octal_str(val)
            self.assertEqual(
                exp,
                mode_to_octal_str(val),
                f"Expected {val} to produce {exp}, got {res}",
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
