#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from antlir.config import antlir_dep

from antlir.nspawn_in_subvol.plugins.tests.rpm_base import RpmNspawnTestBase
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.subvol_utils import with_temp_subvols
from antlir.tests.layer_resource import layer_resource_subvol


class TestImpl:

    _SHADOW_BA_PAIR = (__package__, "shadow-build-appliance")

    _NONDEFAULT_SNAPSHOT_DIR = snapshot_install_dir(
        antlir_dep("rpm:non-default-repo-snapshot-for-tests")
    )

    def test_install_via_default_shadowed_installer(self):
        self._check_yum_dnf_boot_or_not(
            self._PROG,
            "rpm-test-mice",
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "mice 0.1 a\n",
                rb"Installing\s+: rpm-test-mice-0.1-a.x86_64",
            ),
            run_prog_as_is=True,
        )

    def _nondefault_snapshot_prog(self):
        return self._NONDEFAULT_SNAPSHOT_DIR / f"{self._PROG}/bin/{self._PROG}"

    # Installing from a non-default snapshot lets us be sure that we're
    # not accidentally using the default snapshot via the OS wrapper.
    def _check_install_from_nondefault_snapshot(self, prog, extra_args):
        self._check_yum_dnf_boot_or_not(
            # NB: Thanks to `run_prog_as_is`, this can be a path, and not
            # just `yum` or `dnf`.
            prog,
            "rpm-test-cheese",
            extra_args=(
                *extra_args,
                f"--serve-rpm-snapshot={self._NONDEFAULT_SNAPSHOT_DIR}",
            ),
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "cheese 0 0\n",
                rb"Installing\s+: rpm-test-cheese-0-0.x86_64",
            ),
            # Enable default shadowing, and handle `prog` being be a
            # filename, or a in-container absolute path.
            run_prog_as_is=True,
        )

    def test_install_via_manually_shadowed_installer(self):
        # Manually shadow the OS RPM installer, and call it via `PATH`.
        self._check_install_from_nondefault_snapshot(
            self._PROG,
            [
                "--no-shadow-proxied-binaries",
                *(
                    "--shadow-path",
                    self._PROG,
                    self._nondefault_snapshot_prog(),
                ),
            ],
        )

    def test_install_via_nondefault_snapshot(self):
        # Shadow the OS RPM installer, but run the installer wrapper from a
        # different snapshot.  Ensures that even though the OS installer is
        # wrapped, the wrapper from another snapshot still works.
        self._check_install_from_nondefault_snapshot(
            self._nondefault_snapshot_prog(), []
        )

    def test_install_via_nondefault_snapshot_no_shadowing(self):
        # Redundant with other tests: do not shadow the OS installer, run
        # the installer wrapper straight from the snapshot.
        self._check_install_from_nondefault_snapshot(
            self._nondefault_snapshot_prog(), ["--no-shadow-proxied-binaries"]
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
        with dest_subvol.maybe_create_externally():
            self._check_yum_dnf_ret(
                "i will shadow\n",
                rb"Installing\s+: rpm-test-carrot-2-rc0.x86_64",
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
                    # This container will shadow the default RPM installer.
                    run_prog_as_is=True,
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
