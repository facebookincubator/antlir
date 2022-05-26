#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from contextlib import contextmanager
from pwd import struct_passwd
from unittest import mock, TestCase

from antlir.tests.layer_resource import layer_resource

from ..args import _parse_cli_args, PopenArgs
from ..cmd import _extra_nspawn_args_and_env
from ..common import nspawn_version
from ..run import _set_up_run_cli


@contextmanager
def _mocks_for_parse_cli_args():
    with mock.patch(
        "antlir.nspawn_in_subvol.args.pwd.getpwnam"
    ) as getpwnam_mock:
        getpwnam_mock.side_effect = [
            struct_passwd(
                [
                    "pw_name",
                    "pw_passwd",
                    123,
                    123,
                    "pw_gecos",
                    "/test/home",
                    "/test/sh",
                ]
            )
        ]
        yield


@contextmanager
def _mocks_for_extra_nspawn_args(*, artifacts_require_repo):
    with mock.patch(
        "antlir.nspawn_in_subvol.cmd._artifacts_require_repo"
    ) as amrr_mock:
        amrr_mock.side_effect = [artifacts_require_repo]
        yield


class NspawnTestBase(TestCase):
    def setUp(self) -> None:
        # Setup expected stdout line endings depending on the version of
        # systemd-nspawn.  Version 242 'fixed' stdout line endings.  The
        # extra newline for versions < 242 is due to T40936918 mentioned in
        # `run.py`.  It would disappear if we passed `--quiet` to nspawn,
        # but we want to retain the extra debug logging.
        self.nspawn_version = nspawn_version()
        self.maybe_extra_ending = (
            b"\n" if self.nspawn_version.major < 242 else b""
        )

    def _nspawn_in_boot_ret(self, rsrc_pair, argv, **kwargs):
        with _set_up_run_cli(
            ["--layer", layer_resource(*rsrc_pair), *argv]
        ) as cli_setup:
            if "console" in kwargs:
                cli_setup = cli_setup._replace(console=kwargs.pop("console"))
            return cli_setup._run_nspawn(PopenArgs(**kwargs))

    def _nspawn_in(self, rsrc_pair, argv, **kwargs):
        ret, _boot_ret = self._nspawn_in_boot_ret(rsrc_pair, argv, **kwargs)
        return ret

    def _wrapper_args_to_nspawn_args(
        self, argv, *, artifacts_require_repo: bool = False
    ):
        with _mocks_for_parse_cli_args():
            args = _parse_cli_args(argv, allow_debug_only_opts=True)
        with _mocks_for_extra_nspawn_args(
            artifacts_require_repo=artifacts_require_repo
        ):
            # pyre-fixme[16]: `_NspawnOpts` has no attribute `opts`.
            args, _env = _extra_nspawn_args_and_env(args.opts)
            return args
