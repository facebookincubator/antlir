#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
from collections import Counter
from typing import Any, Callable, Generator, List, Optional, Tuple

from antlir.btrfs_diff.coroutine_utils import while_not_exited

from antlir.tests.common import AntlirTestCase


class DeepCopyTestCase(AntlirTestCase):
    """
    If you have a test that builds up some complex object (e.g. `InodeIDMap`,
    `Subvolume`) by following a script, you can use this utility to make
    your test also check that your object is correctly `deepcopy`able.

    To start, you will inject `obj = yield 'step name', obj` throughout your
    test script.
     - Each `yield` point needs a unique name for safety & debuggability.
     - Be sure to keep the set of `yield` points the same, no matter how
       many times your test script runs.

    Then, call `self.check_deepcopy_at_each_step(self.your_test_script)`.

    This will invoke your test script numerous times, `deepcopy`ing the
    object at each of the steps to check that your test script succeeds even
    if it's operating on a copy.  After `deepcopy`-based run, your test
    script will also run with the original object swapped back in at the
    same step.  This ensures that the mutations on the copy did not affect
    the original.

    There are limits to this test -- it only verifies `deepcopy` safety with
    respect to the operations that you perform.  It is very easy to miss
    subtle object aliasing issues in a simple unit-test.  Therefore, this
    test is not a substitute for systematically reasoning about whether each
    part of your object is correctly `deepcopy`able.
    """

    def check_deepcopy_at_each_step(
        self, gen_fn: Callable[[], Generator[Tuple[str, Any], Any, None]]
    ) -> None:
        """
        `gen_fn` makes a generator that yields `(step_name, obj)`, and gets
        sent `obj` or ``deepcopy(obj)`.
        """
        steps = self._check_deepcopy(gen_fn)
        for deepcopy_step, expected_name in enumerate(steps):
            with self.subTest(deepcopy_step=expected_name):
                self.assertEqual(
                    steps,
                    self._check_deepcopy(gen_fn, deepcopy_step, expected_name),
                )

    def _check_deepcopy(
        self,
        gen_fn: Callable[
            [], Generator[Tuple[str, Any], Any, Optional[List[str]]]
        ],
        replace_step=None,
        expected_name=None,
        *,
        _replace_by=None,
    ) -> List[str]:
        """
        Steps through `deepcopy_original`, optionally replacing the ID map
        by deepcopy at a specific step of the test.
        """
        obj = None
        steps = []
        deepcopy_original = None

        with while_not_exited(gen_fn()) as ctx:
            while True:
                step, obj = ctx.send(obj)
                if len(steps) == replace_step:
                    self.assertEqual(expected_name, step)
                    if _replace_by is None:
                        deepcopy_original = obj
                        obj = copy.deepcopy(obj)
                    else:
                        obj = _replace_by
                steps.append(step)

        # Don't repeat step names
        self.assertEqual([], [s for s, n in Counter(steps).items() if n > 1])

        # We just replaced the map with a deepcopy at a specific step.  Now,
        # we run the test one more time up to the same step, and replace the
        # map with the pre-deepcopy original to ensure it has not changed.
        if replace_step is not None and _replace_by is None:
            self.assertIsNotNone(deepcopy_original)
            with self.subTest(deepcopy_original=True):
                self.assertEqual(
                    steps,
                    self._check_deepcopy(
                        gen_fn,
                        replace_step,
                        expected_name,
                        _replace_by=deepcopy_original,
                    ),
                )

        return steps
