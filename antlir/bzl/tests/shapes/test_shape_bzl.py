#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest

from antlir.bzl.tests.shapes.shape_bzl import (
    _check_type,
    _recursive_copy_transform,
    Fail,
    shape,
    struct,
    structs,
)
from antlir.bzl.tests.shapes.target_tagger_helper_bzl import (
    target_tagger_helper,
)


TestUnionType = shape.union_t(bool, int)

target_t = shape.shape(
    __I_AM_TARGET__=True,
    name=str,
    path=shape.path,
)


def shape_from_ctor(ctor):
    return ctor(__internal_get_shape=True)


# useful for assertions that compare the full return value of a shape constructor
def expected_shape(__shape__, **kwargs):
    return struct(__shape__=shape_from_ctor(__shape__), **kwargs)


class TestShapeBzl(unittest.TestCase):
    def setUp(self) -> None:
        self.maxDiff = None
        unittest.util._MAX_LENGTH = 12345

    def test_check_type(self) -> None:
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
            ("/hello/world", shape.path),
            ("@cell//project/path:rule", target_t),
            (":rule", target_t),
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
            ("goodbye", shape.enum("hello", "world")),
            (1, shape.path),
            (2, target_t),
            ("invalid_target", target_t),
            ("also:invalid_target", target_t),
            ("//another//invalid:target", target_t),
            ("nope", shape.union(bool, int)),
        ):
            with self.subTest(x=x, t=t):
                with self.assertRaises(Exception):
                    check_type(x, t)

    def test_shape_with_defaults(self) -> None:
        t = shape.shape(answer=shape.field(int, default=42))
        self.assertEqual(t(), expected_shape(answer=42, __shape__=t))
        self.assertEqual(t(answer=3), expected_shape(answer=3, __shape__=t))

    def test_simple_shape(self) -> None:
        t = shape.shape(answer=int)
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    t(answer=answer)
        with self.assertRaises(Fail):
            t(answer=1, undefined_field="boo")
        actual = t(answer=42)
        expected = expected_shape(answer=42, __shape__=t)
        self.assertEqual(actual, expected)
        # Test the `include_dunder_shape=True` branch.  It isn't actually
        # used for anything (yet), but the `opts` field exists for the sake
        # of making the API clear.
        self.assertEqual(
            _recursive_copy_transform(
                actual,
                shape_from_ctor(t),
                struct(
                    include_dunder_shape=True,
                    on_target_fields="fail",
                ),
            ),
            expected,
        )

    def test_nested_simple_shape(self) -> None:
        nested = shape.shape(answer=int)
        t = shape.shape(nested=nested)
        for answer in ("hello", True, {"a": "b"}):
            with self.subTest(answer=answer):
                with self.assertRaises(Fail):
                    t(nested=shape.new(nested, answer=answer))
        self.assertEqual(
            t(nested=shape.new(nested, answer=42)),
            expected_shape(
                nested=expected_shape(answer=42, __shape__=nested),
                __shape__=t,
            ),
        )

    def test_simple_list(self) -> None:
        t = shape.shape(lst=shape.list(int))
        self.assertEqual(t(lst=[1, 2, 3]).lst, [1, 2, 3])
        with self.assertRaises(Fail):
            t(lst=[1, 2, "3"])

    def test_simple_dict(self) -> None:
        t = shape.shape(dct=shape.dict(str, int))
        self.assertEqual(
            t(dct={"a": 1, "b": 2}),
            expected_shape(dct={"a": 1, "b": 2}, __shape__=t),
        )
        with self.assertRaises(Fail):
            t(dct={"a": "b"})

    def test_enum(self) -> None:
        t = shape.shape(e=shape.enum("hello", "world"))
        self.assertEqual(t(e="world"), expected_shape(e="world", __shape__=t))
        with self.assertRaises(Fail):
            t(e="goodbye")
        with self.assertRaises(Fail):
            shape.shape(e=shape.enum("hello", 42))

    def test_nested_list(self) -> None:
        item_type = shape.shape(answer=int)
        t = shape.shape(lst=shape.list(item_type))
        self.assertEqual(
            t(lst=[shape.new(item_type, answer=42)]),
            expected_shape(
                lst=[expected_shape(__shape__=item_type, answer=42)],
                __shape__=t,
            ),
        )

    def test_nested_dict(self) -> None:
        val_type = shape.shape(answer=int)
        t = shape.shape(dct=shape.dict(str, val_type))
        self.assertEqual(
            t(dct={"a": shape.new(val_type, answer=42)}),
            expected_shape(
                dct={"a": expected_shape(__shape__=val_type, answer=42)},
                __shape__=t,
            ),
        )

    def test_nested_collection_with_shape(self) -> None:
        bottom = shape.shape(answer=int)
        t = shape.shape(dct=shape.dict(str, shape.list(bottom)))
        self.assertEqual(
            t(dct={"a": [shape.new(bottom, answer=42)]}),
            expected_shape(
                dct={"a": [expected_shape(answer=42, __shape__=bottom)]},
                __shape__=t,
            ),
        )

    def test_empty_union_type(self) -> None:
        with self.assertRaises(Fail):
            shape.union()

    def test_nested_union(self) -> None:
        t = shape.shape(
            nested=shape.union_t(shape.union_t(str, int), shape.union_t(bool))
        )
        for v in ("hi", 1, True):
            t(nested=v)

    def test_union_of_shapes(self) -> None:
        s = shape.shape(s=str)
        n = shape.shape(n=int)
        b = shape.shape(b=bool)
        t = shape.shape(u=shape.union(s, n, b))
        for v in (
            s(s="foo"),
            n(n=10),
            b(b=False),
        ):
            t(u=v)

    def test_location_serialization(self) -> None:
        shape_with_target = shape.shape(target=target_t)
        target = shape_with_target(target="//example:target")
        for i in [
            target,
            shape.shape(nested=shape_with_target)(nested=target),
            shape.shape(lst=shape.list(shape_with_target))(
                lst=[target],
            ),
            shape.shape(dct=shape.dict(str, shape_with_target))(
                dct={"a": target},
            ),
            shape.shape(uni=shape.union(int, shape_with_target))(
                uni=target,
            ),
        ]:
            with self.subTest(instance=i):
                ser_err = "cannot safely be serialized"
                # serializing directly to files should be blocked
                with self.assertRaisesRegex(Fail, ser_err):
                    shape.json_file("json", i)
                with self.assertRaisesRegex(Fail, ser_err):
                    shape.python_data(
                        name="py", instance=i, shape_impl=":impl", type_name="t"
                    )
                # serializing to a json string is allowed as the user is
                # implicitly acknowledging that they will do the right thing
                # and not cache the results
                json.loads(shape.do_not_cache_me_json(i))

    def test_as_dict_for_target_tagger(self) -> None:
        targ_t = shape.shape(inner=target_t)
        t = shape.shape(num=int, targ=targ_t)
        i = t(num=5, targ=shape.new(targ_t, inner="//foo:bar"))
        self.assertEqual(
            i,
            expected_shape(
                num=5,
                targ=expected_shape(inner="//foo:bar", __shape__=targ_t),
                __shape__=t,
            ),
        )
        self.assertEqual(
            shape.DEPRECATED_as_dict_for_target_tagger(i),
            # Preserves target paths, but removes `__shape__`.
            {"num": 5, "targ": {"inner": "//foo:bar"}},
        )

    def test_as_target_tagged_dict(self) -> None:
        shape_with_target = shape.shape(target=target_t)
        target = shape_with_target(
            target="//example:target",
        )

        self.assertEqual(
            shape.as_target_tagged_dict(
                target_tagger_helper.new_target_tagger(), target
            ),
            {
                "target": {
                    "path": {"__BUCK_TARGET": "//example:target"},
                }
            },
        )

    def test_as_dict_shallow(self) -> None:
        y = shape.shape(z=int)
        t = shape.shape(x=str, y=y)
        i = t(x="a", y=shape.new(y, z=3))
        self.assertEqual({"x": "a", "y": i.y}, shape.as_dict_shallow(i))

    def test_as_serializable_dict(self) -> None:
        s = shape.shape(z=shape.field(int, optional=True))
        y = shape.shape(z=shape.field(int, optional=True))
        t = shape.shape(
            x=str,
            y=y,
            lst=shape.list(s),
        )
        # Cover the `t.optional and val == None`, and the `val` is set branches
        for z in [3, None]:
            self.assertEqual(
                {
                    "x": "a",
                    "y": {"z": z},
                    "lst": [{"z": z}, {"z": 1}],
                },
                shape.as_serializable_dict(
                    t(
                        x="a",
                        y=shape.new(y, z=z),
                        lst=[shape.new(s, z=z), shape.new(s, z=1)],
                    )
                ),
            )

    def test_target_is_shape(self) -> None:
        t = shape.shape(__I_AM_TARGET__=True)
        self.assertTrue(shape.is_shape(t))

    def test_is_instance(self) -> None:
        y_t = shape.shape(z=int)
        t = shape.shape(x=str, y=y_t)
        i = t(x="a", y=shape.new(y_t, z=3))

        # Good cases
        self.assertTrue(shape.is_any_instance(i))
        self.assertTrue(shape.is_instance(i, t))
        self.assertTrue(shape.is_any_instance(i.y))
        self.assertTrue(shape.is_instance(i.y, y_t))

        # Evil twins of `i`
        s = struct(x="a", y=struct(z=3))
        self.assertEqual(structs.to_dict(s), shape.as_serializable_dict(i))
        d = {"x": "a", "y": {"z": 3}}
        self.assertEqual(d, shape.as_serializable_dict(i))

        # Not a shape instance
        for not_i in [None, d, s, t, y_t]:
            self.assertFalse(shape.is_any_instance(not_i))
            self.assertFalse(shape.is_instance(not_i, t))

        # Instance of the wrong shape
        self.assertFalse(shape.is_instance(i, y_t))
        self.assertFalse(shape.is_instance(i.y, t))

        # Second argument is not a shape
        with self.assertRaisesRegex(Fail, " is not a shape"):
            shape.is_instance(i.y, i.y)

    def test_unionT_typedef(self) -> None:
        self.assertIsNone(_check_type(True, TestUnionType))
        self.assertIsNone(_check_type(False, TestUnionType))
        self.assertIsNone(_check_type(0, TestUnionType))
        self.assertIsNone(_check_type(1, TestUnionType))
        self.assertEqual(
            "foo not matched in union (<class 'bool'>, <class 'int'>): "
            + "expected bool, got foo; expected int, got foo",
            _check_type("foo", TestUnionType),
        )

    def test_no_underscore_fields(self) -> None:
        shape.shape(ohai=int)  # this is fine
        with self.assertRaisesRegex(Fail, " must not start with _:"):
            shape.shape(_ohai=int)  # but the _ ruins everything

    def test_fail_on_dict_coercion(self) -> None:
        inner_t = shape.shape(is_in=shape.dict(str, str, optional=True))
        outer_t = shape.shape(is_out=shape.path, nested=inner_t)
        self.assertEqual(
            expected_shape(
                is_out="/a/path",
                nested=expected_shape(
                    is_in={"hello": "world"},
                    __shape__=inner_t,
                ),
                __shape__=outer_t,
            ),
            outer_t(
                is_out="/a/path",
                nested=shape.new(inner_t, is_in={"hello": "world"}),
            ),
        )
        with self.assertRaisesRegex(Fail, " is not an instance of "):
            outer_t(
                is_out="/a/path",
                # Identical to above, except this is `dict` and not `shape.new`
                nested={"is_in": {"hello": "world"}},
            )

    def test_optional_with_default(self) -> None:
        with self.assertRaisesRegex(
            Fail, "default_value must not be specified with optional"
        ):
            shape.field(str, optional=True, default="def")

    def test_target_and_path_unsupported(self) -> None:
        with self.assertRaisesRegex(Fail, "no longer supported"):
            shape.path()

    # Tetst for target_tagger_helper to get full coverage
    def test_extract_tagged_target(self) -> None:
        self.assertEqual(
            target_tagger_helper.extract_tagged_target(
                {"__BUCK_TARGET": "buck_target"}
            ),
            "buck_target",
        )
        self.assertEqual(
            target_tagger_helper.extract_tagged_target(
                {"__BUCK_LAYER_TARGET": "buck_layer_target"}
            ),
            "buck_layer_target",
        )

    def test_tag_required_target_key(self) -> None:
        tagger = target_tagger_helper.new_target_tagger()

        target = {"target": "target_path"}
        target_tagger_helper.tag_required_target_key(
            tagger, target, "target", False
        )
        self.assertEqual(
            target,
            {"target": {"__BUCK_TARGET": "target_path"}},
        )

        layer = {"layer": "layer_path"}
        target_tagger_helper.tag_required_target_key(
            tagger, layer, "layer", True
        )
        self.assertEqual(
            layer,
            {"layer": {"__BUCK_LAYER_TARGET": "layer_path"}},
        )

        self.assertEqual(tagger.targets, {"target_path": 1, "layer_path": 1})

    def test_target_tagger_to_feature(self) -> None:
        self.assertEqual(
            target_tagger_helper.target_tagger_to_feature(
                target_tagger_helper.new_target_tagger(),
                items=["item"],
                extra_deps=["extra_deps"],
            ),
            struct(items=["item"], deps=["extra_deps"]),
        )

    def test_default_value_sentinel(self) -> None:
        t = shape.shape(value_with_default=shape.field(int, default=42))
        self.assertEqual(
            t(value_with_default=3),
            expected_shape(value_with_default=3, __shape__=t),
        )
        self.assertEqual(
            t(value_with_default=shape.DEFAULT_VALUE),
            expected_shape(value_with_default=42, __shape__=t),
        )
