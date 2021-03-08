#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import glob
import os
import unittest

from antlir.fs_utils import Path


PROV_ROOT = Path("/usr/lib/systemd/system")
ADMIN_ROOT = Path("/etc/systemd/system")
TMPFILES_ROOT = Path("/etc/tmpfiles.d")

# tuple of the form:
# ( <unit name>, <enabled target>, <masked bool>, <dropin name>)
unit_test_specs = [
    ("cheese-file.service", "default.target", False, "cheese-dropin.conf"),
    ("cheese-export.service", "sysinit.target", False, "cheese-dropin.conf"),
    (
        "cheese-export-with-dest.service",
        "default.target",
        False,
        "cheese-dropin-with-dest.conf",
    ),
    ("cheese-generated.service", None, False, "cheese-dropin.conf"),
    ("cheese-source.service", None, True, "cheese-dropin.conf"),
]


def _twant(target):
    """ Make a target name into a '.wants' dir as a Path type."""
    return Path(target + ".wants")


class TestSystemdFeatures(unittest.TestCase):
    def test_units_installed(self):
        for unit, *_ in unit_test_specs:
            unit_file = PROV_ROOT / unit

            self.assertTrue(os.path.exists(unit_file), unit_file)

    def test_units_enabled(self):
        # Get a list of the available .wants dirs for all targets to validate
        available_targets = [
            Path(avail) for avail in glob.glob(PROV_ROOT / "*.wants")
        ]

        # spec[1] is the target name, skip if None
        for unit, target, *_ in unit_test_specs:
            # Make sure it's enabled where it should be
            if target:
                enabled_in_target = PROV_ROOT / _twant(target) / unit

                self.assertTrue(
                    os.path.islink(enabled_in_target), enabled_in_target
                )
                self.assertTrue(
                    os.path.isfile(enabled_in_target), enabled_in_target
                )

            # make sure it's *not* enabled where it shouldn't be
            for avail_target in [
                avail
                for avail in available_targets
                if target and avail.basename() != _twant(target)
            ]:
                unit_in_target_wants = avail_target / unit

                self.assertFalse(
                    os.path.exists(avail_target / unit), unit_in_target_wants
                )

    def test_units_masked(self):
        for unit, _, masked, *_ in unit_test_specs:
            if masked:
                masked_unit = ADMIN_ROOT / unit

                # Yes, systemd (at least in v243) is OK with a relative link
                self.assertEqual(os.readlink(masked_unit), b"../../../dev/null")

        self.assertEqual(
            os.readlink(TMPFILES_ROOT / "testconfig.conf"), b"../../dev/null"
        )

    def test_dropins(self):
        for unit, _, _, dropin in unit_test_specs:
            dropin_file = PROV_ROOT / (unit + ".d") / dropin
            self.assertTrue(os.path.exists(dropin_file), dropin_file)
