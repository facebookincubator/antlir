#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import subprocess
import tempfile
from contextlib import contextmanager
from typing import Iterable
from unittest import skipIf

from antlir.tests.flavor_helpers import get_rpm_installers_supported

from .rpm_base import RpmNspawnTestBase


class TestImpl:
    @contextmanager
    def _write_versionlocks(self, lines: Iterable[str]):
        with tempfile.NamedTemporaryFile(mode="w") as tf:
            tf.write("\n".join(lines) + "\n")
            tf.flush()
            yield tf.name

    def _check_version_lock(self, build_appliance_pair):
        # Version-locking carrot causes a non-latest version to be installed
        # -- compare with `test_yum_with_repo_server`.
        with self._write_versionlocks(
            ["0\trpm-test-carrot\t1\tlockme\tx86_64"]
        ) as vl:
            self._check_yum_dnf_boot_or_not(
                self._PROG,
                "rpm-test-carrot",
                extra_args=(
                    "--snapshot-to-versionlock",
                    self._SNAPSHOT_DIR,
                    vl,
                ),
                check_ret_fn=functools.partial(
                    self._check_yum_dnf_ret,
                    "carrot 1 lockme\n",
                    br"Installing\s+: rpm-test-carrot-1-lockme.x86_64",
                ),
                build_appliance_pair=build_appliance_pair,
            )

    def test_version_lock_build_appliance(self):
        self._check_version_lock((__package__, "build-appliance"))

    def test_version_lock_no_antlir_build_appliance(self):
        self._check_version_lock((__package__, "no-antlir-build-appliance"))

    def test_version_lock_invalid(self):
        def _not_reached(ret):
            raise NotImplementedError

        with self._write_versionlocks(
            ["0\trpm-test-carrot\t3333\tnonesuch\tx86_64"]
        ) as vl, self.assertRaises(subprocess.CalledProcessError):
            self._check_yum_dnf_boot_or_not(
                self._PROG,
                "rpm-test-carrot",
                extra_args=(
                    "--snapshot-to-versionlock",
                    self._SNAPSHOT_DIR,
                    vl,
                ),
                check_ret_fn=_not_reached,
            )


class DnfVersionlockTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "dnf"


@skipIf(
    "yum" not in get_rpm_installers_supported(),
    "yum is not a supported rpm installer",
)
class YumVersionlockTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "yum"
