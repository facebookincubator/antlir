#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import logging
import re
import subprocess
import tempfile
import unittest
from contextlib import contextmanager

from antlir.common import check_popen_returncode

from antlir.nspawn_in_subvol.plugins import launch_repo_servers, server_launcher
from antlir.nspawn_in_subvol.plugins.tests.rpm_base import (
    NspawnTestBase,
    RpmNspawnTestBase,
)
from antlir.tests.flavor_helpers import get_rpm_installers_supported


class TestImpl:
    @contextmanager
    def _check_no_repodata_fetches(self, expected_repomds):
        """
        Ensures that our metadata cache is set up correctly by asserting that
        the repo-server makes no /repodata/ requests.

        Future: we do not yet do anything with cache file mtimes, so this is
        not validating that we correctly set `metadata_expires`.
        """
        with tempfile.NamedTemporaryFile("r") as logfile, subprocess.Popen(
            ["tee", logfile.name], stdout=2, stdin=subprocess.PIPE
        ) as tee, unittest.mock.patch.object(
            server_launcher, "_mockable_popen_for_server"
        ) as mock_popen:
            mock_popen.side_effect = lambda *args, **kwargs: subprocess.Popen(
                *args, **kwargs, stderr=tee.stdin
            )
            yield
            self.assertEqual(
                4,  # {default snapshot, non-default} x {booted, non-booted}
                len(mock_popen.call_args_list),
            )

            repodata_re = re.compile(
                r"DEBUG repo_server\.py .* "
                r'"GET /([^/]*)/repodata/(.*) HTTP/1\.1" 200 -$'
            )
            seen_repomds = set()
            for l in logfile:
                m = repodata_re.match(l)
                if m:
                    # Any other repodata access means our cache is bad.
                    self.assertEqual(m.group(2), "repomd.xml", l)
                    seen_repomds.add(m.group(1))
            self.assertTrue(
                seen_repomds.issubset(expected_repomds),
                (seen_repomds, expected_repomds),
            )
        check_popen_returncode(tee)

    def _check_repo_servers(self, build_appliance):
        # Get basic coverage for our non-trivial debug log code.
        # Note also that `_check_no_repodata_fetches` relies on this.
        launch_repo_servers.log.setLevel(logging.DEBUG)
        with self._check_no_repodata_fetches({"bunny", "dog", "cat", "puppy"}):
            self._check_yum_dnf_boot_or_not(
                self._PROG,
                "rpm-test-mice",
                check_ret_fn=functools.partial(
                    self._check_yum_dnf_ret,
                    "mice 0.1 a\n",
                    rb"Installing\s+: rpm-test-mice-0.1-a.x86_64",
                ),
                # The other case turns off binary shadowing, and runs
                # unwrapped `yum` or `dnf`, which would break caching.
                # Future: we should probably stop testing with unwrapped
                # `yum` / `dnf` entirely.
                run_prog_as_is=True,
                build_appliance_pair=(__package__, build_appliance),
            )

    def test_repo_servers_build_appliance(self):
        self._check_repo_servers("build-appliance")

    def test_repo_servers_no_antlir_build_appliance(self):
        self._check_repo_servers("no-antlir-build-appliance")


class DnfRepoServersTestCase(TestImpl, RpmNspawnTestBase):
    _PROG = "dnf"


class ProxyServerTestCase(NspawnTestBase):
    # This is a basic test that we can spin up the server.
    # Full integration test will be done in chef_solo layer.
    def test_proxy_server(self):
        build_appliance_pair = (__package__, "build-appliance")

        with tempfile.TemporaryFile(
            mode="w+b"
        ) as curl_stdout, tempfile.TemporaryDirectory() as tmpdir:
            self._nspawn_in(
                build_appliance_pair,
                [
                    "--user=root",
                    "--run-proxy-server",
                    f"--forward-fd={curl_stdout.fileno()}",
                    "--",
                    "/bin/sh",
                    "-c",
                    "curl -X GET -I http://localhost:45063/blah -m 3 2>/dev/null \
                    | head -1 > /proc/self/fd/3",
                ],
            )

            curl_stdout.seek(0)

            self.assertEqual(
                curl_stdout.readline().strip(),
                b"HTTP/1.0 404 Unknown route: blah",
            )
