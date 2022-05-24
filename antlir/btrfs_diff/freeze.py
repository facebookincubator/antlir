#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is an analog of `copy.deepcopy`, with the caveat that `freeze` returns
a recursively immutable copy of its argument.  This is achieved by replacing
mutable containers by immutable ones (including all the built-ins, plus
`NamedTuple`) after `freeze`ing every item.  If an object provides the
method `freeze(self, *, _memo, ...)`, we call that instead, and use its
return value.

Analogously to `deepcopy`, the `_memo` keyword argument is used to correctly
copy multiple references to the same object. Unlike `deepcopy`, we make no
provisions for `freeze`ing self-referential structures, because it is
impossible to construct a recursively immutable structure that references
itself.

Future: Once `deepfrozen` is landed, this sort of thing should get nicer.
"""
from collections.abc import Mapping
from enum import Enum
from types import MappingProxyType


# Classes inheriting from this are ignored by freeze().
class DoNotFreeze:
    pass


# pyre-fixme[24]: Generic type `tuple` expects at least 1 type parameter.
class frozendict(Mapping, tuple, DoNotFreeze):
    __slots__ = ()

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __new__(cls, *args, **kwargs):
        return tuple.__new__(cls, (MappingProxyType(dict(*args, **kwargs)),))

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __contains__(self, key):
        return key in tuple.__getitem__(self, 0)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __getitem__(self, key):
        return tuple.__getitem__(self, 0)[key]

    def __len__(self) -> int:
        return len(tuple.__getitem__(self, 0))

    # pyre-fixme[3]: Return type must be annotated.
    def __iter__(self):
        return iter(tuple.__getitem__(self, 0))

    # pyre-fixme[3]: Return type must be annotated.
    def keys(self):
        return tuple.__getitem__(self, 0).keys()

    # pyre-fixme[3]: Return type must be annotated.
    def values(self):
        return tuple.__getitem__(self, 0).values()

    # pyre-fixme[3]: Return type must be annotated.
    def items(self):
        return tuple.__getitem__(self, 0).items()

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def get(self, key, default=None):
        return tuple.__getitem__(self, 0).get(key, default)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __eq__(self, other):
        # pyre-fixme[10]: Name `__class__` is used but not defined.
        if isinstance(other, __class__):
            other = tuple.__getitem__(other, 0)
        return tuple.__getitem__(self, 0).__eq__(other)

    # pyre-fixme[2]: Parameter must be annotated.
    def __ne__(self, other) -> bool:
        return not self == other

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}({repr(dict(tuple.__getitem__(self, 0)))})"
        )

    def __hash__(self) -> int:
        # Although python dictionaries are order preserving,
        # we hash order-insensitive because that's how dict equality works.
        return hash(frozenset(self.items()))  # Future: more efficient hash?


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def freeze(obj, *, _memo=None, **kwargs):
    # Don't bother memoizing primitive types
    if isinstance(obj, (bytes, Enum, float, int, str, type(None))):
        return obj

    if _memo is None:
        _memo = {}

    if id(obj) in _memo:  # Already frozen?
        return _memo[id(obj)]

    if hasattr(obj, "freeze"):
        frozen = obj.freeze(_memo=_memo, **kwargs)
    else:
        # At the moment, I don't have a need for passing extra data into
        # items that live inside containers.  If we're relaxing this, just
        # be sure to add `**kwargs` to each `freeze()` call below.
        assert kwargs == {}, kwargs
        # This is a lame-o way of identifying `NamedTuple`s. Using
        # `deepfrozen` would avoid this kludge.
        if (
            isinstance(obj, tuple)
            and hasattr(obj, "_replace")
            and hasattr(obj, "_fields")
            and hasattr(obj, "_make")
        ):
            # pyre-fixme[16]: `tuple` has no attribute `_make`.
            frozen = obj._make(freeze(i, _memo=_memo) for i in obj)
        elif isinstance(obj, (list, tuple)):
            frozen = tuple(freeze(i, _memo=_memo) for i in obj)
        elif isinstance(obj, dict):
            frozen = frozendict(
                {
                    freeze(k, _memo=_memo): freeze(v, _memo=_memo)
                    for k, v in obj.items()
                }
            )
        elif isinstance(obj, (set, frozenset)):
            frozen = frozenset(freeze(i, _memo=_memo) for i in obj)
        elif isinstance(obj, DoNotFreeze):
            frozen = obj
        else:
            raise NotImplementedError(type(obj))

    _memo[id(obj)] = frozen
    return frozen
