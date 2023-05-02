# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _any(predicate, iterable):
    """
    This expression lazily iterates the container with 0 new allocations.
    In the event that the iterable is empty, it will return False.

    For scenarios like this:

    _ = any([i % 2 == 0 for i in range(100000)])

    The list comprehension would lead to a new list of 100000 booleans,
    and would only end-up checking 1. Replacing it with:

    _ = is_any(lambda i: i % 2 == 0, range(100000))

    would lead to 0 new allocations.
    """
    for i in iterable:
        if predicate(i):
            return True

    return False

def _all(predicate, iterable):
    """
    This expression lazily iterates the container with 0 new allocations.
    In the event that the iterable is empty, it will return False.

    For scenarios like this:

    _ = all([i % 2 == 0 for i in range(100000)])

    The list comprehension would lead to a list of 100000 booleans.
    Replacing it with:

    _ = is_all(lambda i: i % 2 == 0, range(100000))

    would lead to 0 new allocations.
    """
    for i in iterable:
        if not predicate(i):
            return False
    return True

lazy = struct(
    any = _any,
    all = _all,
)
