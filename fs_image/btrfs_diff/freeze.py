#!/usr/bin/env python3
'''
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
'''
from enum import Enum
from types import MappingProxyType


def freeze(obj, *, _memo=None, **kwargs):
    # Don't bother memoizing primitive types
    if isinstance(obj, (bytes, Enum, float, int, str, type(None))):
        return obj

    if _memo is None:
        _memo = {}

    if id(obj) in _memo:  # Already frozen?
        return _memo[id(obj)]

    if hasattr(obj, 'freeze'):
        frozen = obj.freeze(_memo=_memo, **kwargs)
    else:
        # At the moment, I don't have a need for passing extra data into
        # items that live inside containers.  If we're relaxing this, just
        # be sure to add `**kwargs` to each `freeze()` call below.
        assert kwargs == {}, kwargs
        # This is a lame-o way of identifying `NamedTuple`s. Using
        # `deepfrozen` would avoid this kludge.
        if (
            isinstance(obj, tuple) and hasattr(obj, '_replace') and
            hasattr(obj, '_fields') and hasattr(obj, '_make')
        ):
            frozen = obj._make(freeze(i, _memo=_memo) for i in obj)
        elif isinstance(obj, (list, tuple)):
            frozen = tuple(freeze(i, _memo=_memo) for i in obj)
        elif isinstance(obj, dict):
            frozen = MappingProxyType({
                freeze(k, _memo=_memo): freeze(v, _memo=_memo)
                    for k, v in obj.items()
            })
        elif isinstance(obj, (set, frozenset)):
            frozen = frozenset(freeze(i, _memo=_memo) for i in obj)
        else:
            raise NotImplementedError(type(obj))

    _memo[id(obj)] = frozen
    return frozen
