#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile
import textwrap

from antlir.nspawn_in_subvol.tests.base import NspawnTestBase
from antlir.rpm.find_snapshot import snapshot_install_dir


class RpmNspawnTestBase(NspawnTestBase):

    _SNAPSHOT_DIR = snapshot_install_dir("//antlir/rpm:repo-snapshot-for-tests")

    def _yum_or_dnf_install(
        self,
        prog,
        package,
        *,
        extra_args=(),
        # This helper powers two flavors of tests:
        #
        #  - `with_shadowing_wrapper=False`: Directly using the OS RPM
        #    installer with `--config` and `--installroot` -- this may not
        #    be a scenario we end up supporting long-term.
        #
        #  - `with_shadowing_wrapper=True`: The OS RPM installer is shadowed
        #    with its wrapper from the `antlir`-generated RPM repo
        #    snapshot.  This approximates the "RPM installation just works
        #    by default" flow.  Here, we install to `/` instead of
        #    `/target`, because RPM installers currently don't support
        #    updating shadowed files when using `--installroot`, and we want
        #    to test this behavior.
        with_shadowing_wrapper=False,
        build_appliance_pair=(__package__, "build-appliance"),
    ):
        # fmt: off
        maybe_quoted_prog_args = ("" if with_shadowing_wrapper else " ".join([
            f"--config={self._SNAPSHOT_DIR}/{prog}/etc/{prog}/{prog}.conf",
            "--installroot=/target",
        ]))
        # fmt: on
        with tempfile.TemporaryFile(
            mode="w+b"
        ) as yum_dnf_stdout, tempfile.TemporaryFile(mode="w+") as rpm_contents:
            # We don't pipe either stdout or stderr so that both are
            # viewable when running the test interactively.  We use `tee` to
            # obtain a copy of the program's stdout for tests.
            ret = self._nspawn_in(
                build_appliance_pair,
                [
                    "--user=root",
                    f"--serve-rpm-snapshot={self._SNAPSHOT_DIR}",
                    f"--forward-fd={yum_dnf_stdout.fileno()}",  # becomes FD 3
                    f"--forward-fd={rpm_contents.fileno()}",  # becomes FD 4
                    *extra_args,
                    "--",
                    "/bin/sh",
                    "-c",
                    textwrap.dedent(
                        f"""\
                    set -ex
                    install_root={'/' if with_shadowing_wrapper else '/target'}
                    mkdir -p "$install_root"
                    {prog} {maybe_quoted_prog_args} -y install {package} |
                        tee /proc/self/fd/3
                    # We install only 1 RPM, so a glob tells us the filename.
                    # Use `head` instead of `cat` to fail nicer on exceeding 1.
                    head "$install_root"/rpm_test/*.txt >&4
                """
                    ),
                ],
            )
            # Hack up the `CompletedProcess` for ease of testing.
            yum_dnf_stdout.seek(0)
            ret.stdout = yum_dnf_stdout.read()
            rpm_contents.seek(0)
            ret.rpm_contents = rpm_contents.read()
            return ret

    def _check_yum_dnf_ret(self, expected_contents, regex_logline, ret):
        self.assertEqual(0, ret.returncode)
        self.assertEqual(expected_contents, ret.rpm_contents)
        self.assertRegex(ret.stdout, regex_logline)
        self.assertIn(b"Complete!", ret.stdout)

    def _check_yum_dnf_boot_or_not(
        self, prog, package, *, extra_args=(), check_ret_fn=None, **kwargs
    ):
        for boot_args in (["--boot"], []):
            check_ret_fn(
                self._yum_or_dnf_install(
                    prog,
                    package,
                    extra_args=(*extra_args, *boot_args),
                    **kwargs,
                )
            )
