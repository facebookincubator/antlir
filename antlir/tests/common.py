# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from contextlib import contextmanager


class AntlirTestCase(unittest.IsolatedAsyncioTestCase):
    """
    This base class improves test failure output -- use `super().setUp()`.
    Also supplies some testing helpers.
    """

    def setUp(self) -> None:
        # `unittest`'s output shortening makes tests hard to debug, e.g.
        #   i[Mixin(requiresHelper=False, fbpkgs=i[Mi[108 chars]x'])] !=
        #   [Mixin(requiresHelper=False, fbpkgs=i[Mix[100 chars]i[])]
        unittest.util._MAX_LENGTH = 20000  # 250 lines of 80 chars
        self.maxDiff = 20000

    def assert_call_count(self, mock, expected_count) -> None:
        self.assertEqual(
            len(mock.mock_calls),
            expected_count,
            f"Mock had {len(mock.mock_calls)} calls but we expected it to have "
            f"{expected_count}: {mock.mock_calls}",
        )

    def assert_call_equality(self, mock, expected_calls, **kwargs) -> None:
        """Helper to ensure a given mock had *only* the expected calls by also
        asserting the length of the iterable.
        """
        self.assert_call_count(mock, len(expected_calls))
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

    def mock_enter_context(self, patched_ctx_mgr):
        """Returns 'entered' mocked context manager for further mocking.

        Example usage:

        ### Mocking in the following way:
        with mock.patch.object(mod, "attr") as patched:
            self.mock_enter_context(patched).return_value = 123

        ### Will cause the following to be true in the system under test
        from mod import attr

        with attr() as x: assert x == 123
        """
        return patched_ctx_mgr.return_value.__enter__
