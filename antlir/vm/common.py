# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import contextlib
from functools import wraps


def async_wrapper(f):
    """Decorate a function to run in an async event loop."""

    @wraps(f)
    def wrapper(*args, **kwargs):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(f(*args, **kwargs))

    return wrapper


def insertstack(f):
    """
    Decorate an `asynccontextmanager` to insert an `AsyncExitStack` that it can
    use internally.  The `AsyncExitStack` is passed to the wrapped function via
    the `stack=` kwarg
    """

    # TODO: maybe inspect f to make sure it is really an asynccontextmanager?
    @wraps(f)
    async def wrapper(*args, **kwargs):
        async with contextlib.AsyncExitStack() as stack:
            async with f(*args, stack=stack, **kwargs) as r:
                yield r

    return contextlib.asynccontextmanager(wrapper)
