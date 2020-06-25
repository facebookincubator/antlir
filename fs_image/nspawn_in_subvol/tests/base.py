#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from fs_image.tests.layer_resource import layer_resource

from ..args import PopenArgs
from ..common import nspawn_version
from ..run import _set_up_run_cli


class NspawnTestBase(unittest.TestCase):

    def setUp(self):
        # Setup expected stdout line endings depending on the version of
        # systemd-nspawn.  Version 242 'fixed' stdout line endings.  The
        # extra newline for versions < 242 is due to T40936918 mentioned in
        # `run.py`.  It would disappear if we passed `--quiet` to nspawn,
        # but we want to retain the extra debug logging.
        self.nspawn_version = nspawn_version()
        self.maybe_extra_ending = b'\n' if self.nspawn_version < 242 else b''

    def _nspawn_in_boot_ret(self, rsrc_pair, argv, **kwargs):
        with _set_up_run_cli([
            '--layer', layer_resource(*rsrc_pair), *argv,
        ]) as cli_setup:
            if 'boot_console' in kwargs:
                cli_setup = cli_setup._replace(
                    boot_console=kwargs.pop('boot_console')
                )
            return cli_setup._run_nspawn(PopenArgs(**kwargs))

    def _nspawn_in(self, rsrc_pair, argv, **kwargs):
        ret, _boot_ret = self._nspawn_in_boot_ret(rsrc_pair, argv, **kwargs)
        return ret
