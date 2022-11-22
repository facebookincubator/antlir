#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import re
import unittest
from datetime import datetime

from antlir.vcs import rev_id, revision_timestamp


class TestVCS(unittest.TestCase):
    def test_rev_id(self) -> None:
        # Assert it is a 40 char sha-1 looking thing
        self.assertTrue(re.match(r"\b([a-f0-9]{40})\b", rev_id()))

    def test_revision_time_iso8601(self) -> None:
        # We get a datetime instance back from this, so just
        # test that the current (as of the test run) timestamp
        # doesn't go backwards in time.
        # This change was first introduced on Dec 29th, 2021
        # so lets make sure every commit that this test runs
        # against is newer.
        ts = revision_timestamp()
        self.assertTrue(
            # Timezone info comes from the parsed output of
            # the vcs time, to make sure we don't have any
            # weird edge cases due to different offsets
            # construct the "control date" in the same tz
            # as the timestamp under test.
            ts
            > datetime(2021, 12, 29, tzinfo=ts.tzinfo)
        )
