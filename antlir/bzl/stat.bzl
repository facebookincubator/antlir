# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")

def _user(bits):
    return bits << 6

def _group(bits):
    return bits << 3

def _other(bits):
    return bits

def _all(bits):
    return _user(bits) | _group(bits) | _other(bits)

_MAKE_CLASS_MASK = {
    "a": _all,
    "g": _group,
    "o": _other,
    "u": _user,
}

_PERM_BITS = {
    "r": 4,  # 100
    # s and t don't get applied directly to the mode bits, so are handled
    # separately
    "s": 0,
    "t": 0,
    "w": 2,  # 010
    "x": 1,  # 001
}

_EXTRA_PERMS = {
    ("s", "u"): 4,  # 100,
    ("s", "g"): 2,  # 010,
    ("s", "a"): 6,  # 110,
    ("t", "a"): 1,  # 001,
}

def _parse_symbolic(symbolic):
    mode = 0
    if "-" in symbolic or "=" in symbolic:
        fail("symbolic mode strings only support append options ('+')")
    for directive in symbolic.split(","):
        split = directive.split("+")
        if len(split) != 2:
            fail("directive '{}' was not of the form [classes...]+[perms...]".format(split))
        classes, perms = split
        classes = list(classes.elems())
        classes = classes or ["a"]
        perms = list(perms.elems())
        for cl in classes:
            if cl not in _MAKE_CLASS_MASK:
                fail("'{}' is not a recognized class".format(cl))
            for perm in perms:
                if perm not in _PERM_BITS:
                    fail("'{}' is not a recognized permission value".format(perm))
                mode |= _MAKE_CLASS_MASK[cl](_PERM_BITS[perm])
                if perm in ("s", "t"):
                    mode |= (_EXTRA_PERMS.get((perm, cl), 0) << 9)

    return mode

def _mode(mode):
    if types.is_string(mode):
        return _parse_symbolic(mode)
    elif types.is_int(mode):
        return mode
    fail("mode '{}' was neither str nor int".format(mode))

stat = struct(
    mode = _mode,
    parse = _parse_symbolic,
)
