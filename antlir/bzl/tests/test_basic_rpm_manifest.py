# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest

from antlir.fs_utils import Path

EXPECTED_RECORD_KEYS = {"name", "nevra", "os", "patched_cves", "size", "srpm"}
NOT_NONE_RECORD_KEYS = ["name", "nevra", "patched_cves", "size"]
NOT_NONE_NEVRA_KEYS = ["name", "release", "version"]


class RpmManifestTestCase(unittest.TestCase):
    def test_rpm_manifest_structure(self) -> None:
        with Path.resource(__package__, "rpm-manifest.json", exe=False) as manifest:
            with open(manifest, "r") as mf:
                obj = json.load(mf)
            self.assertIsNotNone(obj)
            rpms = obj.get("rpms")
            self.assertIsNotNone(rpms)
            has_rpm_rpm = False
            for record in rpms:
                self.assertTrue(EXPECTED_RECORD_KEYS.issubset(record.keys()))
                for k in NOT_NONE_RECORD_KEYS:
                    self.assertIsNotNone(record.get(k))
                nevra = record.get("nevra", {})
                for k in NOT_NONE_NEVRA_KEYS:
                    self.assertIsNotNone(nevra.get(k))
                if nevra.get("name") == "rpm":
                    has_rpm_rpm = True
            self.assertTrue(has_rpm_rpm)
