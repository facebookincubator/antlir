# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest


def _get_nis_domain() -> str:
    # Cross-check our statically linked tool against the `hostname` RPM
    domains = {
        subprocess.check_output(cmd, text=True).strip()
        for cmd in [["domainname"], ["/build/nis_domainname"]]
    }
    assert len(domains) == 1, domains
    return domains.pop()


class TestSetAntlirNISDomainName(unittest.TestCase):
    def test_set_domainname(self):
        magic_name = "AntlirNotABuildStep"

        # First test that `run_test.py` correctly sets it by default.
        self.assertEqual(magic_name, _get_nis_domain())

        # Now set it to something else, and ensure that our statically
        # linked binary re-sets it, just to test it directly.

        subprocess.check_call(["domainname", "NOT" + magic_name])
        self.assertNotEqual(magic_name, _get_nis_domain())

        subprocess.check_call(["/build/nis_domainname", "set"])
        self.assertEqual(magic_name, _get_nis_domain())
