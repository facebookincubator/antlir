#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import glob
import os
import unittest
from dataclasses import dataclass
from typing import List, Optional

from antlir.fs_utils import Path


PROV_ROOT = Path("/usr/lib/systemd/system")
ADMIN_ROOT = Path("/etc/systemd/system")
USER_PROV_ROOT = Path("/usr/lib/systemd/user")
TMPFILES_ROOT = Path("/etc/tmpfiles.d")


@dataclass(frozen=True)
class SystemdUnitTestSpec:
    name: str
    installed_root: Path = PROV_ROOT
    enabled_target: Optional[str] = None
    target_dep_type: Optional[str] = None
    enabled_link_name: Optional[str] = None
    dropin_name: Optional[str] = None
    is_masked: bool = False


unit_test_specs: List[SystemdUnitTestSpec] = [
    SystemdUnitTestSpec(
        "cheese-file.service",
        enabled_target="default.target",
        target_dep_type="wants",
        dropin_name="cheese-dropin.conf",
    ),
    SystemdUnitTestSpec(
        "cheese-requires.service",
        enabled_target="example.target",
        target_dep_type="requires",
    ),
    SystemdUnitTestSpec(
        "cheese-export.service",
        enabled_target="sysinit.target",
        target_dep_type="wants",
        dropin_name="cheese-dropin.conf",
    ),
    SystemdUnitTestSpec(
        "cheese-export-with-dest.service",
        enabled_target="default.target",
        target_dep_type="wants",
        dropin_name="cheese-dropin-with-dest.conf",
    ),
    SystemdUnitTestSpec(
        "cheese-source.service",
        dropin_name="cheese-dropin.conf",
        is_masked=True,
    ),
    SystemdUnitTestSpec(
        "cheese-user.service",
        installed_root=USER_PROV_ROOT,
        enabled_target="default.target",
        target_dep_type="wants",
    ),
    SystemdUnitTestSpec(
        "cheese-template@.service",
        enabled_target="default.target",
        target_dep_type="wants",
        enabled_link_name="cheese-template@foo.service",
    ),
]


def _tdep(target, dep):
    """Make a target name into a '.wants/requires' dir as a Path type."""
    return Path(target + "." + dep)


class TestSystemdFeatures(unittest.TestCase):
    def test_units_installed(self) -> None:
        for unit in unit_test_specs:
            unit_file = unit.installed_root / unit.name

            self.assertTrue(os.path.exists(unit_file), unit_file)

    def test_units_enabled(self) -> None:
        for unit in unit_test_specs:
            # Get a list of available .wants dirs for all targets to validate
            available_targets = [
                Path(avail)
                for avail in glob.glob(unit.installed_root / "*.wants")
                + glob.glob(unit.installed_root / "*.requires")
            ]

            # Make sure it's enabled where it should be
            if unit.enabled_target:
                link_name = (
                    unit.enabled_link_name if unit.enabled_link_name else unit.name
                )
                enabled_in_target = (
                    unit.installed_root
                    / _tdep(unit.enabled_target, unit.target_dep_type)
                    / link_name
                )

                self.assertTrue(os.path.islink(enabled_in_target), enabled_in_target)
                self.assertTrue(os.path.isfile(enabled_in_target), enabled_in_target)

            # make sure it's *not* enabled where it shouldn't be
            for avail_target in [
                avail
                for avail in available_targets
                if unit.enabled_target
                and (
                    avail.basename() != _tdep(unit.enabled_target, "wants")
                    and avail.basename() != _tdep(unit.enabled_target, "requires")
                )
            ]:
                # pyre-fixme[61]: `link_name` is undefined, or not always defined.
                unit_in_target_wants = avail_target / link_name

                self.assertFalse(
                    os.path.exists(unit_in_target_wants), unit_in_target_wants
                )

    def test_units_masked(self) -> None:
        for unit in unit_test_specs:
            if unit.is_masked:
                masked_unit = ADMIN_ROOT / unit.name

                self.assertEqual(os.readlink(masked_unit), b"/dev/null")

        self.assertEqual(os.readlink(TMPFILES_ROOT / "testconfig.conf"), b"/dev/null")

    def test_dropins(self) -> None:
        for unit in unit_test_specs:
            if unit.dropin_name is None:
                continue
            dropin_file = PROV_ROOT / (unit.name + ".d") / unit.dropin_name
            self.assertTrue(os.path.exists(dropin_file), dropin_file)
