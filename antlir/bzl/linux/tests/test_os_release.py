#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import csv
import os
import re
import unittest
from datetime import datetime

from antlir.fs_utils import Path

CENTOS = int(os.environ["CENTOS"])


class OsReleaseTest(unittest.TestCase):
    def test_os_release(self) -> None:
        """Verify that the os-release file properly built."""

        with Path("/usr/lib/os-release").open() as f:
            reader = csv.reader(f, delimiter="=")
            os_release = dict(reader)

        self.assertEqual(
            os_release["NAME"],
            "CentOS Stream",
        )
        self.assertEqual(
            os_release["ID"],
            "centos",
        )
        self.assertEqual(os_release["VERSION"], str(CENTOS))
        self.assertEqual(
            os_release["VARIANT"],
            "Test",
        )
        # Validate the Pretty Name has the name and says it's a local rev
        self.assertEqual(
            os_release["PRETTY_NAME"],
            f"CentOS Stream {CENTOS} Test (local)",
        )

        # For tests we will never have build_info properly provided
        self.assertEqual(
            os_release["IMAGE_ID"],
            "local",
        )

        # Validate the API Version rendering
        self.assertEqual(os_release["API_VER_BAR"], "22")
        self.assertEqual(os_release["API_VER_FOO_QUX"], "7")

    def test_vcs(self) -> None:
        """Verify that VCS info is correctly included"""

        rev_id_regex = r"\b([a-f0-9]{40})\b"

        with Path("/usr/lib/os-release-vcs").open() as f:
            reader = csv.reader(f, delimiter="=")
            os_release = dict(reader)

        # Validate the Pretty Name has the names + a valid vcs rev
        self.assertTrue(
            re.match(
                rf"CentOS Stream {CENTOS} Test \({rev_id_regex}\)",
                os_release["PRETTY_NAME"],
            ),
            os_release["PRETTY_NAME"],
        )

        # Validate the second part of the BUILD_ID is a vcs rev
        self.assertTrue(re.match(rev_id_regex, os_release["IMAGE_VCS_REV"]))

        try:
            datetime.strptime(os_release["IMAGE_VCS_REV_TIME"], "%Y-%m-%dT%H:%M:%S%z")
        except Exception as e:
            self.fail(
                f"Can't parse revision_time_iso8601 {os_release['IMAGE_VCS_REV_TIME']} as date: {e}"
            )

    def test_custom_name(self) -> None:
        """Verify that a custom 'os_name' is correctly included"""

        with Path("/usr/lib/os-release-name").open() as f:
            reader = csv.reader(f, delimiter="=")
            os_release = dict(reader)

        self.assertEqual(
            "Antlir Test",
            os_release["NAME"],
        )

        self.assertEqual(
            f"Antlir Test {CENTOS} Foo (local)",
            os_release["PRETTY_NAME"],
        )
