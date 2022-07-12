# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest

from antlir.bzl.fail_with_message import log, log_failure_message

from antlir.fs_utils import Path


class FailWithMessageTestCase(unittest.TestCase):
    def test_fail_with_message_logs_error(self) -> None:
        msg_to_log = "TEST FAILURE MSG"
        with self.assertLogs(log, level="ERROR") as log_ctx:
            log_failure_message(msg_to_log)
            self.assertIn(msg_to_log, str(log_ctx.output))

    def test_fail_with_message_e2e(self) -> None:
        msg_to_log = "TEST FAILURE MSG"
        with Path.resource(
            __package__, "fail-with-message", exe=True
        ) as binary:
            res = subprocess.run(
                [binary, "--message", msg_to_log],
                check=True,
                capture_output=True,
            )
            self.assertIn(msg_to_log, res.stderr.decode())
