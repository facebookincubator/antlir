#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import csv
import re
import unittest

from antlir.fs_utils import Path


class OsReleaseTest(unittest.TestCase):
    def test_os_release(self) -> None:
        """Verify that the os-release file properly built."""

        rev_id_regex = r"\b([a-f0-9]{40})\b"
        rev_ts_regex = r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?$"

        with Path("/usr/lib/os-release").open() as f:
            reader = csv.reader(f, delimiter="=")
            os_release = dict(reader)

        self.assertEqual(
            os_release["NAME"],
            "AntlirTest",
        )
        self.assertEqual(
            os_release["ID"],
            "antlirtest",
        )
        self.assertEqual(
            os_release["VARIANT"],
            "Test",
        )
        # Validate the Pretty Name has the names + a valid vcs rev
        self.assertTrue(
            re.match(
                rf"AntlirTest\ Test\ \({rev_id_regex}\)",
                os_release["PRETTY_NAME"],
            )
        )

        # For tests we will never have build_info properly provided
        self.assertEqual(
            os_release["IMAGE_ID"],
            "local",
        )

        # Validate the second part of the BUILD_ID is a vcs rev
        self.assertTrue(re.match(rev_id_regex, os_release["IMAGE_VCS_REV"]))

        # Version and image_timestamp should be an ISO8601
        self.assertTrue(
            re.match(rev_ts_regex, os_release["VERSION"]),
        )
        self.assertTrue(
            re.match(rev_ts_regex, os_release["IMAGE_VCS_REV_TIME"]),
        )

        # Validate the API Version rendering
        self.assertEqual(os_release["API_VER_BAR"], "22")
        self.assertEqual(os_release["API_VER_FOO_QUX"], "7")
