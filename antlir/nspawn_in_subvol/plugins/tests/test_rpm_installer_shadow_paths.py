#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.temp_subvolumes import with_temp_subvols

from .rpm_base import RpmNspawnTestBase


class TestImpl:

    _SHADOW_BA_PAIR = (__package__, "shadow-build-appliance")

    def _shadow_prog_args(self):
        return (
            "--shadow-path",
            self._PROG,
            self._SNAPSHOT_DIR / f"{self._PROG}/bin/{self._PROG}",
        )

    def test_install(self):
        self._check_yum_dnf_boot_or_not(
            self._PROG,
            "rpm-test-mice",
            extra_args=self._shadow_prog_args(),
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "mice 0.1 a\n",
                br"Installing\s+: rpm-test-mice-0.1-a.x86_64",
            ),
            with_shadowing_wrapper=True,
        )

    def _check_shadow_ba(self):
        ba_original = layer_resource_subvol(*self._SHADOW_BA_PAIR)
        self.assertEqual(
            "shadow me\n", ba_original.path("/rpm_test/carrot.txt").read_text()
        )

    # This cannot use `_check_yum_dnf_boot_or_not` because we need a
    # separate destination subvolume for each of the subtests.
    @with_temp_subvols
    def _check_update_shadowed_file(self, temp_subvols, *, boot):
        self._check_shadow_ba()

        dest_subvol = temp_subvols.caller_will_create("shadow_ba")
        # Fixme: formalize this pattern from `test_non_ephemeral_snapshot`
        dest_subvol._exists = True

        self._check_yum_dnf_ret(
            "i will shadow\n",
            br"Installing\s+: rpm-test-carrot-2-rc0.x86_64",
            self._yum_or_dnf_install(
                self._PROG,
                "rpm-test-carrot",
                extra_args=(
                    *(["--boot"] if boot else []),
                    *self._shadow_prog_args(),
                    *(
                        "--shadow-path",
                        "/rpm_test/carrot.txt",
                        "/i_will_shadow",
                    ),
                    f"--snapshot-into={dest_subvol.path()}",
                ),
                with_shadowing_wrapper=True,
                build_appliance_pair=self._SHADOW_BA_PAIR,
            ),
        )
        # The RPM installation worked as expected despite the shadow
        self.assertEqual(
            "carrot 2 rc0\n",
            dest_subvol.path("/rpm_test/carrot.txt").read_text(),
        )

        self._check_shadow_ba()

    def test_update_shadowed_file_booted(self):
        self._check_update_shadowed_file(boot=True)

    def test_update_shadowed_file_non_booted(self):
        self._check_update_shadowed_file(boot=False)


class DnfRpmInstallerShadowPathsTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "dnf"


class YumRpmInstallerShadowPathsTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "yum"
