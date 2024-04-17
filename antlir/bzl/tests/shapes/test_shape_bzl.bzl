# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

def _assert_eq(actual, expected, msg = None):
    if actual != expected:
        fail("assert_eq failed: {} != {}".format(actual, expected) + (": " + msg) if msg else "")

simple = shape.shape(answer = int)

def _test_simple():
    _assert_eq(simple(answer = 42).answer, 42)

nested = shape.shape(nested = simple)

def _test_simple_nested():
    _assert_eq(nested(nested = simple(answer = 42)).nested.answer, 42)

with_defaults = shape.shape(answer = shape.field(int, default = 42))

def _test_defaults():
    _assert_eq(with_defaults().answer, 42)
    _assert_eq(with_defaults(answer = 3).answer, 3)

def test_shape_bzl():
    _test_simple()
    _test_simple_nested()
    _test_defaults()
