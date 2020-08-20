#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from .rpm_base import RpmNspawnTestBase


class TestImpl:
    def test_repo_servers(self):
        self._check_yum_dnf_boot_or_not(
            self._PROG,
            "rpm-test-mice",
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                "mice 0.1 a\n",
                br"Installing\s+: rpm-test-mice-0.1-a.x86_64",
            ),
        )


class DnfRepoServersTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "dnf"


class YumRepoServersTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "yum"
