#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest

from .shape_bzl import Fail, _check_type, _codegen_shape, shape, struct


class TestShapeBzl(unittest.TestCase):
    def setUp(self):
        self.maxDiff = None

    def test_check_type(self):
        def check_type(x, t):
            res = _check_type(x, t)
            assert res is None, res

        for x, t in (
            (2, int),
            (False, bool),
            ("hello", str),
            ("hello", shape.field(str)),
            ("hello", shape.field(str, optional=True)),
            (None, shape.field(str, optional=True)),
            ({"a": "b"}, shape.dict(str, str)),
            ("/hello/world", shape.path()),
            ("@cell//project/path:rule", shape.target()),
            (":rule", shape.target()),
            (":rule", shape.layer()),
        ):
            with self.subTest(x=x, t=t):
                check_type(x, t)

        for x, t in (
            (2, bool),
            ("hello", int),
            (True, shape.field(str)),
            ("hello", shape.field(int, optional=True)),
            ({"a": 1}, shape.dict(str, str)),
            ({1: "b"}, shape.dict(str, str)),
            ("nope", shape.dict(str, str)),
            ("nope", shape.list(str)),
            ("nope", shape.tuple(str)),
            (1, shape.path()),
            (2, shape.target()),
            ("invalid_target", shape.target()),
            ("also:invalid_target", shape.target()),
            ("invalid_layer", shape.layer()),
        ):
            with self.subTest(x=x, t=t):
                with self.assertRaises(Exception):
                    check_type(x, t)

    def test_shape_with_defaults(self):
        t = shape.shape(answer=shape.field(int, default=42))
        self.assertEqual(shape.new(t), struct(answer=42))
        self.assertEqual(shape.new(t, answer=3), struct(answer=3))

    def test_simple_shape(self):
        t = shape.shape(answer=int)
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    shape.new(t, answer=answer)
        self.assertEqual(shape.new(t, answer=42), struct(answer=42))

    def test_nested_simple_shape(self):
        t = shape.shape(nested=shape.shape(answer=int))
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    shape.new(t, nested=shape.new(t.nested, answer=answer))
        self.assertEqual(
            shape.new(t, nested=shape.new(t.nested, answer=42)),
            struct(nested=struct(answer=42)),
        )

    def test_simple_list(self):
        t = shape.shape(lst=shape.list(int))
        self.assertEqual(shape.new(t, lst=[1, 2, 3]), struct(lst=[1, 2, 3]))
        with self.assertRaises(Fail):
            shape.new(t, lst=[1, 2, "3"])

    def test_simple_dict(self):
        t = shape.shape(dct=shape.dict(str, int))
        self.assertEqual(
            shape.new(t, dct={"a": 1, "b": 2}), struct(dct={"a": 1, "b": 2})
        )
        with self.assertRaises(Fail):
            shape.new(t, dct={"a": "b"})

    def test_simple_tuple(self):
        t = shape.shape(tup=shape.tuple(str, int))
        self.assertEqual(
            shape.new(t, tup=("hello", 1)), struct(tup=("hello", 1))
        )
        with self.assertRaises(Fail):
            shape.new(t, tup=("hello", "2"))

    def test_nested_list(self):
        t = shape.shape(lst=shape.list(shape.shape(answer=int)))
        self.assertEqual(
            shape.new(t, lst=[shape.new(t.lst.item_type, answer=42)]),
            struct(lst=[struct(answer=42)]),
        )

    def test_nested_dict(self):
        t = shape.shape(dct=shape.dict(str, shape.shape(answer=int)))
        self.assertEqual(
            shape.new(t, dct={"a": shape.new(t.dct.item_type[1], answer=42)}),
            struct(dct={"a": struct(answer=42)}),
        )

    def test_nested_collection_with_shape(self):
        bottom = shape.shape(answer=int)
        t = shape.shape(dct=shape.dict(str, shape.list(bottom)))
        self.assertEqual(
            shape.new(t, dct={"a": [shape.new(bottom, answer=42)]}),
            struct(dct={"a": [struct(answer=42)]}),
        )

    def test_codegen(self):
        # the generated code is tested in test_shape.py, but this is our
        # opportunity to test it as text
        nested = shape.shape(inner=bool)
        t = shape.shape(
            hello=str,
            world=shape.field(str, optional=True),
            answer=shape.field(int, default=42),
            file=shape.path(),
            location=shape.target(),
            nested=shape.field(nested, default=shape.new(nested, inner=True)),
            dct=shape.dict(str, str),
            lst=shape.list(int),
            tup=shape.tuple(bool, int, str),
            nested_lst=shape.list(shape.shape(inner_lst=bool)),
            nested_dct=shape.dict(str, shape.shape(inner_dct=bool)),
            dct_of_lst_of_shape=shape.dict(
                str, shape.list(shape.shape(answer=int))
            ),
        )
        code = "\n".join(_codegen_shape(t, "shape"))
        self.assertEqual(
            code,
            """class shape(Shape):
  __GENERATED_SHAPE__ = True
  hello: str
  world: Optional[str]
  answer: int = 42
  file: Path
  location: Target
  class _2UNYP6wnsQdfqkEJEKDmwaEjpoGm8_8tlX3BIHNt_sQ(Shape):
    __GENERATED_SHAPE__ = True
    inner: bool
  nested: _2UNYP6wnsQdfqkEJEKDmwaEjpoGm8_8tlX3BIHNt_sQ = _2UNYP6wnsQdfqkEJEKDmwaEjpoGm8_8tlX3BIHNt_sQ(**{'inner': True})
  dct: Mapping[str, str]
  lst: Tuple[int, ...]
  tup: Tuple[bool, int, str]
  class _NRjZd_W5gdohVquSVb4iz3YwOUh_dtUKmLgIHb4h_m0(Shape):
    __GENERATED_SHAPE__ = True
    inner_lst: bool
  nested_lst: Tuple[_NRjZd_W5gdohVquSVb4iz3YwOUh_dtUKmLgIHb4h_m0, ...]
  class _ZOuD9rKDIF_qItVd5ib0hWFXRe4UKS1dPdfwP_rEGl0(Shape):
    __GENERATED_SHAPE__ = True
    inner_dct: bool
  nested_dct: Mapping[str, _ZOuD9rKDIF_qItVd5ib0hWFXRe4UKS1dPdfwP_rEGl0]
  class __wWKYeDaABhdYr5uCMdTzSclY0GG2FUB0OvzGPn42OE(Shape):
    __GENERATED_SHAPE__ = True
    answer: int
  dct_of_lst_of_shape: Mapping[str, Tuple[__wWKYeDaABhdYr5uCMdTzSclY0GG2FUB0OvzGPn42OE, ...]]""",
        )

    def test_location_serialization(self):
        target_t = shape.shape(target=shape.target())
        layer_t = shape.shape(layer=shape.layer())
        nested_target_t = shape.shape(nested=target_t)
        list_with_targets_t = shape.shape(lst=shape.list(target_t))
        dict_with_targets_t = shape.shape(dct=shape.dict(str, target_t))
        tuple_with_targets_t = shape.shape(tup=shape.tuple(str, target_t))
        shapes = [
            (target_t, shape.new(target_t, target="//example:target")),
            (layer_t, shape.new(layer_t, layer="//example:layer")),
            (
                nested_target_t,
                shape.new(
                    nested_target_t,
                    nested=shape.new(target_t, target="//example:target"),
                ),
            ),
            (
                list_with_targets_t,
                shape.new(
                    list_with_targets_t,
                    lst=[shape.new(target_t, target="//example:target")],
                ),
            ),
            (
                dict_with_targets_t,
                shape.new(
                    dict_with_targets_t,
                    dct={"a": shape.new(target_t, target="//example:target")},
                ),
            ),
            (
                tuple_with_targets_t,
                shape.new(
                    tuple_with_targets_t,
                    tup=("a", shape.new(target_t, target="//example:target")),
                ),
            ),
        ]

        for t, i in shapes:
            with self.subTest(shape=t, instance=i):
                # serializing directly to files should be blocked
                with self.assertRaises(Fail):
                    shape.json_file("json", i, t)
                with self.assertRaises(Fail):
                    shape.python_data("py", i, t)
                # serializing to a json string is allowed as the user is
                # implicitly acknowledging that they will do the right thing
                # and not cache the results
                json.loads(shape.do_not_cache_me_json(i, t))
