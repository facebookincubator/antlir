#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Python runtime component of shape.bzl. This file is not meant to be used
# directly, instead it contains supporting implementations for bzl/shape.bzl.
# See that file for motivations and usage documentation.

import collections
import dataclasses
import re
import typing


class ShapeMeta(type):
    def __repr__(cls):
        fields = ", ".join(
            f"{f.name}={getattr(f.type, '__name__', str(f.type))}"
            for f in dataclasses.fields(cls)
        )
        # hide the module since it is ultimately not important, shapes are not
        # meant to be constructed in pure-python code, only in bzl macros
        fields = fields.replace(cls.__module__ + ".shape(", "shape(")
        # typing.Optional reduces down to a Union with NoneType, so show the
        # original intent
        fields = re.sub(
            r"typing.Union\[(.*?), NoneType\]", r"typing.Optional[\1]", fields
        )
        # also hide typing. prefixes on things
        fields = fields.replace("typing.", "")
        return f"shape({fields})"


def __shape_instance_repr(self):
    fields = ", ".join(
        f"{f.name}={getattr(self, f.name)}" for f in dataclasses.fields(self)
    )
    return f"shape({fields})"


def __check_type(var, typ):
    try:
        if isinstance(var, typ):
            return
        else:
            raise TypeError(f"expected {typ}, got {type(var)}")
    except TypeError:
        pass
    if hasattr(typ, "__origin__"):
        if typ.__origin__ in (collections.abc.Mapping, dict):
            key_type, val_type = typ.__args__
            for key, val in var.items():
                __check_type(key, key_type)
                __check_type(val, val_type)
            return
        if typ.__origin__ in (collections.abc.Sequence, list):
            item_type = typ.__args__[0]
            for item in var:
                __check_type(item, item_type)
            return
        if typ.__origin__ == tuple:
            item_types = typ.__args__
            __check_type(var, tuple)
            if len(var) != len(item_types):
                raise TypeError(
                    f"expected tuple of {len(item_types)} elements, "
                    f"got {len(var)}"
                )
            for item, item_type in zip(var, item_types):
                __check_type(item, item_type)
            return
        if typ.__origin__ == typing.Union:
            union_types = set(typ.__args__)
            if type(None) in union_types:
                union_types.remove(type(None))
                if var is None:
                    return
            for t in union_types:
                try:
                    __check_type(var, t)
                    return
                except TypeError:
                    pass
            raise TypeError(f"{var} was none of the union types {typ.__args__}")
    # fail open
    raise TypeError(f"Unable to validate that {var} has type {typ}")


def __maybe_downcast(var, typ):
    """
    Intelligently downcast to the appropriate shape types from untyped
    dictionary inputs.
    """
    if hasattr(typ, "_SHAPE_DATACLASS"):
        return typ(**var)
    if hasattr(typ, "__origin__"):
        if typ.__origin__ in (collections.abc.Sequence, list):
            item_type = typ.__args__[0]
            return [__maybe_downcast(e, item_type) for e in var]
        if typ.__origin__ in (collections.abc.Mapping, dict):
            # keys can only be primitives, not shapes themselves
            _key_type, val_type = typ.__args__
            return {k: __maybe_downcast(v, val_type) for k, v in var.items()}
        if typ.__origin__ == tuple:
            item_types = typ.__args__
            return tuple(
                __maybe_downcast(e, item_type)
                for e, item_type in zip(var, item_types)
            )
        if typ.__origin__ == typing.Union:
            union_types = set(typ.__args__)
            if type(None) in union_types:
                union_types.remove(type(None))
                if var is None:
                    return None
            for t in union_types:
                maybe = __maybe_downcast(var, t)
                if maybe != var:
                    return maybe
    return var


def __post_init(self):
    """
    Simple runtime type checking and casting implementation, sufficient
    enough for the generated code from `shape.bzl`, but perhaps not for
    general consumption compared to something like `pydantic`.
    """
    # convert nested shape types into the appropriate classes
    for field in dataclasses.fields(self):
        val = getattr(self, field.name)
        val = __maybe_downcast(val, field.type)
        object.__setattr__(self, field.name, val)

    # dataclass __init__ will have already handled non-optional fields, so only
    # check if the types are correct
    for field in dataclasses.fields(self):
        val = getattr(self, field.name)
        __check_type(val, field.type)


def shape_dataclass(cls):
    cls._SHAPE_DATACLASS = None
    cls.__post_init__ = __post_init
    cls.__repr__ = __shape_instance_repr
    dc = dataclasses.dataclass(frozen=True)(cls)
    cls.__name__ = repr(cls)
    cls.__qualname__ = repr(cls)
    return dc
