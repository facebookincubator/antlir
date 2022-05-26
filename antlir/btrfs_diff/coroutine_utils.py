#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Start with `help(while_not_exited)`"
from contextlib import contextmanager
from typing import Any, Iterator


class CoroutineContext:
    """
    `while_not_exited` returns this context. You can access two members:

    coroutine:  Only available inside the `with`
    result:  Only available after the `with`
    """

    result: Any = None  # see `test_throw_from_sender` for why this is set.

    def __init__(self, coroutine) -> None:
        self.coroutine = coroutine

    # Syntax sugar -- remember that `send` and `throw` will exit the `with`
    # block if they cause the generator to exit.

    def close(self):
        return self.coroutine.close()

    def send(self, value):
        return self.coroutine.send(value)

    def throw(self, ex):
        return self.coroutine.throw(ex)


class GeneratorExitWithResult(Exception):
    """
    If your coroutine needs to handle `GeneratorExit` and produce a result,
    it cannot just `return result`, since Python appears **NOT** to insert
    `result` into the subsequent `StopIteration`.  So, instead, you can
    catch `GeneratorExit`, and re-raise `GeneratorExitWithResult(result)`.

    Keep in mind that if the sender calls `.close` before the initial send,
    your coroutine will automatically return None.
    """


@contextmanager
def while_not_exited(coroutine) -> Iterator[CoroutineContext]:
    """
    A helper for sending a sequence of values to a co-routine, and capturing
    its final return value.  Please review the test to review the exact data
    flow -- it is not very intuitive.

    Best practices:

      - Avoid using the return value of `None` as a sentinel. It is not
        possible to distinguish a `None` returned by your coroutine from one
        injected by the language.  See `test_throw_from_sender` for a demo.

      - `ctx.send(None)` **before** `ctx.close()`, or `ctx.result` will
        be set to `None`, since the coroutine will never have run.

    Usage pattern:

        with while_not_exited(your_coroutine(...)) as ctx:
            # Communicate with the coroutine as usual:
            #  - `yielded_val = ctx.send(some_value)`
            #  - `ctx.close()` is implicit when you exit the `with`, but you
            #    may call it explicitly if desired.

    The coroutine gets destroyed when you exit the `with`.

    `ctx.result` will be set to the its return value, or `None` if the
    coroutine did not return a value.  If your coroutine needs to return
    values after `.close()` events, look at `GeneratorExitWithResult`.
    """
    ctx = CoroutineContext(coroutine)
    try:
        yield ctx
        # We don't need to `.close()` when exiting the `with` due to an
        # exception -- the coroutine will have already exited.  On the other
        # hand, if the **sender** exited before the coroutine did, we should
        # `.close()` right away, because the next line will handle
        # `GeneratorExitWithResult`.
        coroutine.close()
    except GeneratorExitWithResult as ex:
        (ctx.result,) = ex.args
    except StopIteration as ex:
        ctx.result = ex.value
    finally:
        del ctx.coroutine  # Don't leak `.coroutine' outside the `with`
