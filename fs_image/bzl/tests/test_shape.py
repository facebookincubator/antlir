#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from collections.abc import Sequence
from typing import Optional, Union

from pydantic import ValidationError

from .example_loader import example as shape
from .pyfile_shape import data


class TestShape(unittest.TestCase):
    def test_load(self):
        """load happy path from json_file, python_file and code"""

        def expected(e):
            self.assertEqual(42, e.answer)
            self.assertEqual(True, e.field)
            self.assertEqual(True, e.nested.inner)
            self.assertEqual(True, e.tp[2].nested)
            self.assertEqual({"hello": "world"}, e.dct)
            self.assertEqual(11, e.dct_w_shape["apollo"].number)

        e = shape.read_resource(__package__, "example.json")
        expected(e)

        # same results when instantiating in code
        e = shape(
            answer=42,
            field=True,
            nested={"inner": True},
            tp=(True, 42, {"nested": True}),
            dct={"hello": "world"},
            dct_w_shape={"apollo": {"number": 11}},
        )
        expected(e)

        # same results when importing the pre-instantiated version
        expected(data)

    def test_missing_parameters(self):
        """fail to instantiate shape with missing required fields"""
        with self.assertRaises(TypeError):
            shape(answer=42)

    def test_tuple_invalid_element_type(self):
        """fail to instantiate shape with one invalid tuple element"""
        with self.assertRaises(ValidationError):
            shape(
                answer=42,
                field=True,
                nested={"inner": True},
                tp=(True, 42, {"nested": 42}),
                dct={"hello": "world"},
                dct_w_shape={"apollo": {"number": 11}},
            )

    def test_list_invalid_element_type(self):
        """fail to instantiate shape with one invalid list element"""
        with self.assertRaises(ValidationError):
            shape(
                answer=42,
                field=True,
                nested={"inner": True},
                tp=(True, 42, {"nested": 42}),
                lst=[{"id": 0}, {"oops": 1}],
                dct={"hello": "world"},
                dct_w_shape={"apollo": {"number": 11}},
            )

    def test_typehints(self):
        """check type hints on generated classes"""

        def _deep_typehints(obj):
            hints = {}
            for key, val in getattr(obj, "__annotations__", {}).items():
                if hasattr(val, "__annotations__"):
                    hints[key] = _deep_typehints(val)
                else:
                    hints[key] = val
            return hints

        # simple type hints can easily be compared
        hints = _deep_typehints(shape)
        expected = {
            "answer": int,
            "field": bool,
            "say_hi": Optional[str],
            "nested": {"inner": bool},
        }
        self.assertEqual(
            expected, {k: v for k, v in hints.items() if k in expected}
        )
        # heterogenous tuple with nested shape is a little trickier
        self.assertEqual(hints["tp"].__origin__, Union)
        self.assertEqual(hints["tp"].__args__[1], type(None))
        self.assertEqual(hints["tp"].__args__[0].__origin__, tuple)
        self.assertEqual(len(hints["tp"].__args__[0].__args__), 3)
        self.assertEqual(hints["tp"].__args__[0].__args__[:2], (bool, int))
        self.assertEqual(
            hints["tp"].__args__[0].__args__[2].__annotations__,
            {"nested": bool},
        )
        # likewise with the list
        self.assertEqual(hints["lst"].__origin__, Union)
        self.assertEqual(hints["lst"].__args__[1], type(None))
        self.assertEqual(hints["lst"].__args__[0].__origin__, Sequence)
        self.assertEqual(len(hints["lst"].__args__[0].__args__), 1)
        self.assertEqual(
            hints["lst"].__args__[0].__args__[0].__annotations__, {"id": int}
        )
