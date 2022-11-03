#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import shlex
import tempfile
import textwrap

from antlir.config import antlir_dep
from antlir.nspawn_in_subvol.tests.base import NspawnTestBase
from antlir.rpm.find_snapshot import snapshot_install_dir


class RpmNspawnTestBase(NspawnTestBase):

    _SNAPSHOT_DIR = snapshot_install_dir(antlir_dep("rpm:repo-snapshot-for-tests"))

    def _yum_or_dnf_install(
        self,
        prog,
        package,
        *,
        extra_args=(),
        build_appliance_pair=(__package__, "build-appliance"),
        # The `install_root` and `run_prog_as_is` args are here so that we
        # can test various scenarios where {prog} is shadowed by our
        # `yum-dnf-from-snapshot` wrapper.  These are scenarios where to the
        # user it appears that the OS RPM installer "just works" with the
        # default RPM snapshot for that installer.
        #
        # We may want to phase out `run_prog_as_is=False` entirely, because
        # this implies directly using the OS-provided RPM installer with our
        # snapshot's `--config`.  We do not necessarily want to support this
        # scenario long-term, because it stops us from controlling the
        # runtime environment of the RPM installer.
        install_root="/target",
        # The semantics of this are: just run `prog` (via `PATH` if a
        # basename, as a path otherwise).  We also won't automatically serve
        # a snapshot in the container, or disable installer shadowing.
        # Fixme: this should really go away, and be done explicitly by the
        # callsites that currently rely on the `False` branch.
        run_prog_as_is=False,
    ):
        maybe_config_arg = (
            (f"--config={self._SNAPSHOT_DIR}/{prog}/etc/{prog}/{prog}.conf")
            if not run_prog_as_is
            else ""
        )

        test_sh_script = textwrap.dedent(
            f"""\
            set -ex
            install_root={shlex.quote(install_root)}
            mkdir -p "$install_root"
            {prog} -y --installroot="$install_root" {maybe_config_arg} \\
                install {package} | tee /proc/self/fd/3
            # We install only 1 RPM, so a glob tells us the filename.
            # Use `head` instead of `cat` to fail nicer on exceeding 1.
            head "$install_root"/rpm_test/*.txt >&4
            """
        )

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
                    *(
                        []
                        if run_prog_as_is
                        else [
                            "--no-shadow-proxied-binaries",
                            f"--serve-rpm-snapshot={self._SNAPSHOT_DIR}",
                        ]
                    ),
                    f"--forward-fd={yum_dnf_stdout.fileno()}",  # becomes FD 3
                    f"--forward-fd={rpm_contents.fileno()}",  # becomes FD 4
                    *extra_args,
                    "--",
                    "/bin/sh",
                    "-c",
                    test_sh_script,
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
