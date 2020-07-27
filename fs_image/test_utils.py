#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from contextlib import contextmanager


class FsImageTestUtilsMixin:
    """Use as a mixin with a class inheriting unittest.TestCase to provide some
    useful helpers.
    """

    def assert_call_equality(self, mock, expected_calls, **kwargs):
        """Helper to ensure a given mock had *only* the expected calls by also
        asserting the length of the iterable.
        """
        self.assertEqual(len(mock.mock_calls), len(expected_calls))
        mock.assert_has_calls(expected_calls, **kwargs)

    @contextmanager
    def patch_ctx_mgr(self, to_patch):
        """Mocks a context manager by returning the 'entered' object. To use,
        pass in an unstarted patch.

        Example:
        with patch_ctx_mgr(mock.patch.object(mod, "attr")) as patched:
            ...
        """
        with to_patch as patched:
            yield patched.return_value.__enter__.return_value
