#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import unittest

from antlir.buck.buck_label.buck_label_py import Label


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_fails_on_unqualified_labels(self) -> None:
        """
        Fail on unqualified target labels.
        """
        with self.assertRaises(ValueError):
            Label("//in/fo:unqual")

    def test_str(self) -> None:
        """
        Well-formed label can be converted to string, and used compatibly with
        strings
        """
        l = Label("cell//path/to:target (cfg//a:b)")
        self.assertEqual(Label, type(l))
        self.assertEqual("cell//path/to:target (cfg//a:b)", str(l))
        self.assertEqual("cell//path/to:target (cfg//a:b)", l)
        self.assertEqual(l, l)
        self.assertEqual(hash("cell//path/to:target (cfg//a:b)"), hash(l))
        map = {"cell//path/to:target (cfg//a:b)": True}
        self.assertIn(l, map)
        self.assertIn("cell//path/to:target (cfg//a:b)", map)
        map = {l: True}
        self.assertIn(l, map)
        self.assertIn("cell//path/to:target (cfg//a:b)", map)

    def test_unconfigured(self) -> None:
        """
        Unconfigured labels should work too, to play nicely with buck1
        """
        c = Label("cell//path/to:target (cfg//a:b)")
        u = Label("cell//path/to:target")
        self.assertEqual(c.unconfigured, u)
        self.assertNotEqual(c, u)
        self.assertEqual("cell//path/to:target", c.unconfigured)

    def test_properties(self) -> None:
        """
        Components are exposed as Python properties
        """
        l = Label("cell//path/to:target (cfg//a:b)")
        self.assertEqual("cell", l.cell)
        self.assertEqual("path/to", l.package)
        self.assertEqual("target", l.name)
        self.assertEqual("cfg//a:b", l.config)
        self.assertIsNone(l.unconfigured.config)
