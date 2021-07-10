#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest

from .shape_bzl import (
    Fail,
    _check_type,
    _codegen_shape,
    shape,
    struct,
    structs,
    _recursive_copy_transform,
)


TestUnionType = shape.union_t(bool, int)


class TestShapeBzl(unittest.TestCase):
    def setUp(self):
        self.maxDiff = None
        unittest.util._MAX_LENGTH = 12345

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
            ("world", shape.enum("hello", "world")),
            ("/hello/world", shape.path()),
            ("@cell//project/path:rule", shape.target()),
            (":rule", shape.target()),
            (1, shape.union(str, int)),
            ("hello", shape.union(str, int)),
            ("hello", shape.union_t(str, int)),
            ("hello", shape.field(shape.union_t(str, int))),
            ("hello", shape.union(str, int, optional=True)),
            (None, shape.union(str, int, optional=True)),
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
            ("goodbye", shape.enum("hello", "world")),
            (1, shape.path()),
            (2, shape.target()),
            ("invalid_target", shape.target()),
            ("also:invalid_target", shape.target()),
            ("nope", shape.union(bool, int)),
        ):
            with self.subTest(x=x, t=t):
                with self.assertRaises(Exception):
                    check_type(x, t)

    def test_shape_with_defaults(self):
        t = shape.shape(answer=shape.field(int, default=42))
        self.assertEqual(shape.new(t), struct(answer=42, __shape__=t))
        self.assertEqual(shape.new(t, answer=3), struct(answer=3, __shape__=t))

    def test_simple_shape(self):
        t = shape.shape(answer=int)
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    shape.new(t, answer=answer)
        with self.assertRaises(Fail):
            shape.new(t, answer=1, undefined_field="boo")
        actual = shape.new(t, answer=42)
        expected = struct(answer=42, __shape__=t)
        self.assertEqual(actual, expected)
        # Test the `include_dunder_shape=True` branch.  It isn't actually
        # used for anything (yet), but the `opts` field exists for the sake
        # of making the API clear.
        self.assertEquals(
            _recursive_copy_transform(
                actual,
                t,
                struct(
                    include_dunder_shape=True,
                    on_target_fields="fail",
                ),
            ),
            expected,
        )

    def test_nested_simple_shape(self):
        t = shape.shape(nested=shape.shape(answer=int))
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    shape.new(t, nested=shape.new(t.nested, answer=answer))
        self.assertEqual(
            shape.new(t, nested=shape.new(t.nested, answer=42)),
            struct(
                nested=struct(answer=42, __shape__=t.nested),
                __shape__=t,
            ),
        )

    def test_simple_list(self):
        t = shape.shape(lst=shape.list(int))
        self.assertEqual(
            shape.new(t, lst=[1, 2, 3]), struct(lst=[1, 2, 3], __shape__=t)
        )
        with self.assertRaises(Fail):
            shape.new(t, lst=[1, 2, "3"])

    def test_simple_dict(self):
        t = shape.shape(dct=shape.dict(str, int))
        self.assertEqual(
            shape.new(t, dct={"a": 1, "b": 2}),
            struct(dct={"a": 1, "b": 2}, __shape__=t),
        )
        with self.assertRaises(Fail):
            shape.new(t, dct={"a": "b"})

    def test_simple_tuple(self):
        t = shape.shape(tup=shape.tuple(str, int))
        self.assertEqual(
            shape.new(t, tup=("hello", 1)),
            struct(tup=("hello", 1), __shape__=t),
        )
        with self.assertRaisesRegex(Fail, "item 1:"):
            shape.new(t, tup=("hello", "2"))
        with self.assertRaisesRegex(Fail, "length of .* does not match"):
            shape.new(t, tup=("hello",))

    def test_enum(self):
        t = shape.shape(e=shape.enum("hello", "world"))
        self.assertEqual(
            shape.new(t, e="world"),
            struct(e="world", __shape__=t),
        )
        with self.assertRaises(Fail):
            shape.new(t, e="goodbye")
        with self.assertRaises(Fail):
            shape.shape(e=shape.enum("hello", 42))

    def test_nested_list(self):
        t = shape.shape(lst=shape.list(shape.shape(answer=int)))
        self.assertEqual(
            shape.new(t, lst=[shape.new(t.lst.item_type, answer=42)]),
            struct(
                lst=[struct(__shape__=t.lst.item_type, answer=42)],
                __shape__=t,
            ),
        )

    def test_nested_dict(self):
        t = shape.shape(dct=shape.dict(str, shape.shape(answer=int)))
        self.assertEqual(
            shape.new(t, dct={"a": shape.new(t.dct.item_type[1], answer=42)}),
            struct(
                dct={"a": struct(__shape__=t.dct.item_type[1], answer=42)},
                __shape__=t,
            ),
        )

    def test_nested_collection_with_shape(self):
        bottom = shape.shape(answer=int)
        t = shape.shape(dct=shape.dict(str, shape.list(bottom)))
        self.assertEqual(
            shape.new(t, dct={"a": [shape.new(bottom, answer=42)]}),
            struct(
                dct={"a": [struct(answer=42, __shape__=bottom)]},
                __shape__=t,
            ),
        )

    def test_empty_union_type(self):
        with self.assertRaises(Fail):
            shape.union()

    def test_nested_union(self):
        t = shape.shape(
            nested=shape.union_t(shape.union_t(str, int), shape.union_t(bool))
        )
        for v in ("hi", 1, True):
            shape.new(t, nested=v)

    def test_union_of_shapes(self):
        s = shape.shape(s=str)
        n = shape.shape(n=int)
        b = shape.shape(b=bool)
        t = shape.shape(u=shape.union(s, n, b))
        for v in (
            shape.new(s, s="foo"),
            shape.new(n, n=10),
            shape.new(b, b=False),
        ):
            shape.new(t, u=v)

    def test_codegen(self):
        # the generated code is tested in test_shape.py, but this is our
        # opportunity to test it as text
        nested = shape.shape(inner=bool)
        t = shape.shape(
            hello=str,
            world=shape.field(str, optional=True),
            answer=shape.field(int, default=42),
            enum=shape.enum("hello", "world"),
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
            union_of_things=shape.union(int, str),
        )
        code = "\n".join(_codegen_shape(t, "shape"))
        self.assertEqual(
            code,
            """class shape(Shape):
  __GENERATED_SHAPE__ = True
  hello: str
  world: Optional[str] = None
  answer: int = 42
  class Hello_World(Enum):
    HELLO = 'hello'
    WORLD = 'world'
  enum: Hello_World
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
  dct_of_lst_of_shape: Mapping[str, Tuple[__wWKYeDaABhdYr5uCMdTzSclY0GG2FUB0OvzGPn42OE, ...]]
  union_of_things: Union[int, str]""",  # noqa: E501
        )

    def test_codegen_with_empty_union_type(self):
        for t in (shape.shape(empty_list=[]), shape.shape(empty_tuple=())):
            with self.assertRaises(Fail):
                _codegen_shape(t)

    def test_location_serialization(self):
        target_t = shape.shape(target=shape.target())
        target = shape.new(target_t, target="//example:target")
        for i in [
            target,
            shape.new(shape.shape(nested=target_t), nested=target),
            shape.new(
                shape.shape(lst=shape.list(target_t)),
                lst=[target],
            ),
            shape.new(
                shape.shape(dct=shape.dict(str, target_t)),
                dct={"a": target},
            ),
            shape.new(
                shape.shape(tup=shape.tuple(str, target_t)),
                tup=("a", target),
            ),
            shape.new(
                shape.shape(uni=shape.union(int, target_t)),
                uni=target,
            ),
        ]:
            with self.subTest(instance=i):
                ser_err = "cannot safely be serialized"
                # serializing directly to files should be blocked
                with self.assertRaisesRegex(Fail, ser_err):
                    shape.json_file("json", i)
                with self.assertRaisesRegex(Fail, ser_err):
                    shape.python_data("py", i)
                # serializing to a json string is allowed as the user is
                # implicitly acknowledging that they will do the right thing
                # and not cache the results
                json.loads(shape.do_not_cache_me_json(i))

    def test_as_dict_for_target_tagger(self):
        t = shape.shape(num=int, targ=shape.shape(inner=shape.target()))
        i = shape.new(t, num=5, targ=shape.new(t.targ, inner="//foo:bar"))
        self.assertEqual(
            i,
            struct(
                num=5,
                targ=struct(inner="//foo:bar", __shape__=t.targ),
                __shape__=t,
            ),
        )
        self.assertEqual(
            shape.as_dict_for_target_tagger(i),
            # Preserves target paths, but removes `__shape__`.
            {"num": 5, "targ": {"inner": "//foo:bar"}},
        )

    def test_as_dict_shallow(self):
        t = shape.shape(x=str, y=shape.shape(z=int))
        i = shape.new(t, x="a", y=shape.new(t.y, z=3))
        self.assertEqual({"x": "a", "y": i.y}, shape.as_dict_shallow(i))

    def test_as_serializable_dict(self):
        t = shape.shape(x=str, y=shape.shape(z=shape.field(int, optional=True)))
        # Cover the `t.optional and val == None`, and the `val` is set branches
        for z in [3, None]:
            self.assertEqual(
                {"x": "a", "y": {"z": z}},
                shape.as_serializable_dict(
                    shape.new(t, x="a", y=shape.new(t.y, z=z)),
                ),
            )

    def test_is_instance(self):
        t = shape.shape(x=str, y=shape.shape(z=int))
        i = shape.new(t, x="a", y=shape.new(t.y, z=3))

        # Good cases
        self.assertTrue(shape.is_any_instance(i))
        self.assertTrue(shape.is_instance(i, t))
        self.assertTrue(shape.is_any_instance(i.y))
        self.assertTrue(shape.is_instance(i.y, t.y))

        # Evil twins of `i`
        s = struct(x="a", y=struct(z=3))
        self.assertEqual(structs.to_dict(s), shape.as_serializable_dict(i))
        d = {"x": "a", "y": {"z": 3}}
        self.assertEqual(d, shape.as_serializable_dict(i))

        # Not a shape instance
        for not_i in [None, d, s, t, t.y]:
            self.assertFalse(shape.is_any_instance(not_i))
            self.assertFalse(shape.is_instance(not_i, t))

        # Instance of the wrong shape
        self.assertFalse(shape.is_instance(i, t.y))
        self.assertFalse(shape.is_instance(i.y, t))

        # Second argument is not a shape
        with self.assertRaisesRegex(Fail, " is not a shape"):
            shape.is_instance(i.y, i.y)

    def test_unionT_typedef(self):
        self.assertIsNone(_check_type(True, TestUnionType))
        self.assertIsNone(_check_type(False, TestUnionType))
        self.assertIsNone(_check_type(0, TestUnionType))
        self.assertIsNone(_check_type(1, TestUnionType))
        self.assertEqual(
            "foo not matched in union (<class 'bool'>, <class 'int'>): "
            + "expected bool, got foo; expected int, got foo",
            _check_type("foo", TestUnionType),
        )

    def test_no_underscore_fields(self):
        shape.shape(ohai=int)  # this is fine
        with self.assertRaisesRegex(Fail, " must not start with _:"):
            shape.shape(_ohai=int)  # but the _ ruins everything

    def test_fail_on_dict_coercion(self):
        inner_t = shape.shape(is_in=shape.dict(str, str, optional=True))
        outer_t = shape.shape(is_out=shape.path(), nested=inner_t)
        self.assertEqual(
            struct(
                is_out="/a/path",
                nested=struct(
                    is_in={"hello": "world"},
                    __shape__=inner_t,
                ),
                __shape__=outer_t,
            ),
            shape.new(
                outer_t,
                is_out="/a/path",
                nested=shape.new(inner_t, is_in={"hello": "world"}),
            ),
        )
        with self.assertRaisesRegex(Fail, " is not an instance of "):
            shape.new(
                outer_t,
                is_out="/a/path",
                # Identical to above, except this is `dict` and not `shape.new`
                nested={"is_in": {"hello": "world"}},
            )
