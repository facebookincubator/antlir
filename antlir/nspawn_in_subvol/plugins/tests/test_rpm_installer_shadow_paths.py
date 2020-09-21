#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.temp_subvolumes import with_temp_subvols

from .rpm_base import RpmNspawnTestBase


class TestImpl:

    _SHADOW_BA_PAIR = (__package__, "shadow-build-appliance")

    def test_install_via_default_shadowed_installer(self):
        self._check_yum_dnf_boot_or_not(
            self._PROG,
            "rpm-test-mice",
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "mice 0.1 a\n",
                br"Installing\s+: rpm-test-mice-0.1-a.x86_64",
            ),
            is_os_installer_wrapped=True,
        )

    def test_install_via_manual_shadowed_installer(self):
        # Use a non-default snapshot for our manual shadowing, so that we are
        # sure that we're not seeing the effects of default snapshotting.
        snapshot_dir = snapshot_install_dir(
            "//antlir/rpm:non-default-repo-snapshot-for-tests"
        )
        self._check_yum_dnf_boot_or_not(
            self._PROG,
            "rpm-test-cheese",
            extra_args=(
                *(
                    "--shadow-path",
                    self._PROG,
                    snapshot_dir / f"{self._PROG}/bin/{self._PROG}",
                ),
                # NB: these are the same as in  `_yum_or_dnf_install` in the
                # `is_os_installer_wrapped=False` branch.
                "--no-shadow-proxied-binaries",
                f"--serve-rpm-snapshot={snapshot_dir}",
            ),
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "cheese 0 0\n",
                br"Installing\s+: rpm-test-cheese-0-0.x86_64",
            ),
            is_os_installer_wrapped=True,
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
                    *(
                        "--shadow-path",
                        "/rpm_test/carrot.txt",
                        "/i_will_shadow",
                    ),
                    f"--snapshot-into={dest_subvol.path()}",
                ),
                # This will uses the default installer shadowing.
                is_os_installer_wrapped=True,
                # Our RPM installer wrapper doesn't support updating
                # shadowed files with `--installroot` other than `/`.
                install_root="/",
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
