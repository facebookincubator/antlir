#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from .rpm_base import RpmNspawnTestBase


class YumDnfVersionlockTestCase(RpmNspawnTestBase):
    def test_yum_with_repo_server(self):
        self._check_yum_dnf_boot_or_not(
            "yum",
            "rpm-test-carrot",
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "carrot 2 rc0\n",
                b"Package rpm-test-carrot.x86_64 0:2-rc0 will be installed",
            ),
        )

    def test_dnf_with_repo_server(self):
        self._check_yum_dnf_boot_or_not(
            "dnf",
            "rpm-test-mice",
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "mice 0.1 a\n",
                b"Installing       : rpm-test-mice-0.1-a.x86_64",
            ),
        )
