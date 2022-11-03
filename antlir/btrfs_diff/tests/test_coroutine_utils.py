#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import Any, List

from antlir.btrfs_diff.coroutine_utils import GeneratorExitWithResult, while_not_exited


class CoroutineTestError(Exception):
    pass


class SendToCoroutineTestCase(unittest.TestCase):
    events: List[Any] = []

    def demo_coroutine(self, coroutine_steps: int):
        try:
            for i in range(coroutine_steps):
                sent_i = yield i
                self.assertEqual(i, sent_i)
                self.events.append(("sent", sent_i))
            return "finished loop"  # noqa: B901
        except GeneratorExit:
            raise GeneratorExitWithResult("coroutine closed")

    def send_to_demo(self, coroutine_steps: int, close_on_step: int, *, do_close: bool):
        with while_not_exited(self.demo_coroutine(coroutine_steps)) as ctx:
            self.events = []
            self.events.append(("initialized", ctx.send(None)))
            for i in range(close_on_step):
                self.events.append(("yielded", ctx.send(i)))
            if do_close:
                # Shows that our helper handles `.close()` in the `with`
                ctx.close()
        self.assertFalse(hasattr(ctx, "coroutine"))
        return ctx.result, self.events

    def test_while_not_exited(self) -> None:
        for do_close in [False, True]:

            def send_to_demo(coroutine_steps: int, close_on_step: int):
                return self.send_to_demo(
                    coroutine_steps, close_on_step, do_close=do_close
                )

            self.assertEqual(send_to_demo(0, 0), ("finished loop", []))

            self.assertEqual(send_to_demo(0, 1), ("finished loop", []))
            self.assertEqual(send_to_demo(0, 2), ("finished loop", []))

            init = [("initialized", 0)]
            self.assertEqual(send_to_demo(1, 0), ("coroutine closed", init))
            self.assertEqual(send_to_demo(2, 0), ("coroutine closed", init))

            sent0 = init + [("sent", 0)]
            for i in range(1, 5):
                self.assertEqual(send_to_demo(1, i), ("finished loop", sent0))

            yielded1 = sent0 + [("yielded", 1)]
            for i in range(2, 5):
                self.assertEqual(send_to_demo(i, 1), ("coroutine closed", yielded1))

            sent2 = yielded1 + [("sent", 1), ("yielded", 2), ("sent", 2)]
            for i in range(3, 8):
                self.assertEqual(send_to_demo(3, i), ("finished loop", sent2))

            yielded3 = sent2 + [("yielded", 3)]
            for i in range(4, 8):
                self.assertEqual(send_to_demo(i, 3), ("coroutine closed", yielded3))

    def yield_and_raise_coroutine(self):
        self.events.append("yielding")
        v = yield "init"
        self.events.append(("received", v))
        raise CoroutineTestError

    def raise_immediately_coroutine(self):
        raise CoroutineTestError
        yield "init"  # we need a `yield` to make this a generator
        self.fail("not reached")

    def test_coroutines_that_raise(self) -> None:
        with while_not_exited(self.yield_and_raise_coroutine()) as ctx:
            self.events = []
            self.assertEqual("init", ctx.send(None))
            self.assertEqual(["yielding"], self.events)
            with self.assertRaises(CoroutineTestError):
                ctx.send("cat")
            self.assertEqual(["yielding", ("received", "cat")], self.events)
        self.assertIsNone(ctx.result)  # The coroutine never returned.

        with while_not_exited(self.raise_immediately_coroutine()) as ctx:
            with self.assertRaises(CoroutineTestError):
                ctx.send(None)
        self.assertIsNone(ctx.result)  # The coroutine never returned.

    def test_throw_from_sender(self) -> None:
        with while_not_exited(self.demo_coroutine(100)) as ctx:
            with self.assertRaises(CoroutineTestError):
                ctx.throw(CoroutineTestError)
            # This illustrates an unfortunate event -- even though the
            # coroutine never returned, the `send`-after-`close` will raise
            # a `StopIteration(None)`, which caused us to record a return
            # value of `None`.  For this reason, `while_not_exited` always
            # defaults `result` to None in its `finally`.
            ctx.send("cat")
            self.fail("the coroutine should have exited, skipping this")
        self.assertIsNone(ctx.result)

    def test_sender_closes_immediately(self) -> None:
        with while_not_exited(self.demo_coroutine(5)) as ctx:
            ctx.close()
        self.assertIsNone(ctx.result)  # The coroutine never ran.


if __name__ == "__main__":
    unittest.main()
