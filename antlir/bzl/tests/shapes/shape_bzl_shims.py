#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Small set of shims that when imported by shape.bzl makes it valid python that
can be imported and unit tested.
Only the necessary functions used by shape.bzl have been shimmed.
This technically would allow code that is not valid starlark, but that is
covered by the other tests that exercise shape.bzl within buck.
"""
import base64
import dataclasses
import hashlib


def __dataclass_eq(left, right):
    if not dataclasses.is_dataclass(right):
        return False
    return dataclasses.asdict(left) == dataclasses.asdict(right)


def struct(**kwargs):
    cls = dataclasses.make_dataclass(
        "struct",
        [(k, type(v)) for k, v in kwargs.items()],
        namespace={"__eq__": __dataclass_eq},
    )
    return cls(**kwargs)


def load(_file, *_symbols):
    pass


class Fail(Exception):
    pass


def fail(msg, attr=None):
    if attr:  # pragma: no cover
        msg = f"{attr}: {msg}"
    raise Fail(msg)


class structs(object):
    @staticmethod
    def is_struct(x):
        return dataclasses.is_dataclass(x)

    @staticmethod
    def to_dict(x):
        return dataclasses.asdict(x)


class types(object):
    @staticmethod
    def is_bool(x):
        return type(x) == bool

    @staticmethod
    def is_int(x):
        return type(x) == int

    @staticmethod
    def is_string(x):
        return type(x) == str

    @staticmethod
    def is_dict(x):
        return type(x) == dict

    @staticmethod
    def is_list(x):
        return type(x) == list

    @staticmethod
    def is_tuple(x):
        return type(x) == tuple


def sha256_b64(s):
    m = hashlib.sha256()
    m.update(s.encode())
    return base64.b64encode(m.digest(), altchars=b"-_").rstrip(b"=").decode()
