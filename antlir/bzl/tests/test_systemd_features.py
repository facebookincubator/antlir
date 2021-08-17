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
# ( <unit name>, <enabled target>, <target dep type>, <masked bool>, <dropin name>)
unit_test_specs = [
    (
        "cheese-file.service",
        "default.target",
        "wants",
        False,
        "cheese-dropin.conf",
    ),
    (
        "cheese-requires.service",
        "example.target",
        "requires",
        False,
        None,
    ),
    (
        "cheese-export.service",
        "sysinit.target",
        "wants",
        False,
        "cheese-dropin.conf",
    ),
    (
        "cheese-export-with-dest.service",
        "default.target",
        "wants",
        False,
        "cheese-dropin-with-dest.conf",
    ),
    ("cheese-generated.service", None, None, False, "cheese-dropin.conf"),
    ("cheese-source.service", None, None, True, "cheese-dropin.conf"),
]


def _tdep(target, dep):
    """Make a target name into a '.wants/requires' dir as a Path type."""
    return Path(target + "." + dep)


class TestSystemdFeatures(unittest.TestCase):
    def test_units_installed(self):
        for unit, *_ in unit_test_specs:
            unit_file = PROV_ROOT / unit

            self.assertTrue(os.path.exists(unit_file), unit_file)

    def test_units_enabled(self):
        # Get a list of the available .wants dirs for all targets to validate
        available_targets = [
            Path(avail)
            for avail in glob.glob(PROV_ROOT / "*.wants")
            + glob.glob(PROV_ROOT / "*.requires")
        ]

        # spec[1] is the target name, skip if None
        for unit, target, target_dep, *_ in unit_test_specs:
            # Make sure it's enabled where it should be
            if target:
                enabled_in_target = PROV_ROOT / _tdep(target, target_dep) / unit

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
                if target
                and (
                    avail.basename() != _tdep(target, "wants")
                    and avail.basename() != _tdep(target, "requires")
                )
            ]:
                unit_in_target_wants = avail_target / unit

                self.assertFalse(
                    os.path.exists(avail_target / unit), unit_in_target_wants
                )

    def test_units_masked(self):
        for unit, _, _, masked, *_ in unit_test_specs:
            if masked:
                masked_unit = ADMIN_ROOT / unit

                # Yes, systemd (at least in v243) is OK with a relative link
                self.assertEqual(os.readlink(masked_unit), b"../../../dev/null")

        self.assertEqual(
            os.readlink(TMPFILES_ROOT / "testconfig.conf"), b"../../dev/null"
        )

    def test_dropins(self):
        for unit, _, _, _, dropin in unit_test_specs:
            if not dropin:
                continue
            dropin_file = PROV_ROOT / (unit + ".d") / dropin
            self.assertTrue(os.path.exists(dropin_file), dropin_file)
