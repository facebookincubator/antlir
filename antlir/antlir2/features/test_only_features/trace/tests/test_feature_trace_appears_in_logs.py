# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import unittest


class TestFeatureTraceAppearsInLogs(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_feature_trace_appears_in_logs(self) -> None:
        logs = importlib.resources.read_text(__package__, "logs")
        self.assertIn("This feature-internal trace should appear in log files", logs)
