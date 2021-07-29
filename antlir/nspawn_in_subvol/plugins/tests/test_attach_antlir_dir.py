#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.nspawn_in_subvol.tests.base import NspawnTestBase
from antlir.subvol_utils import with_temp_subvols
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol

from ..attach_antlir_dir import ANTLIR_DIR

_SRC_SUBVOL_PAIR = (__package__, "no-antlir-layer")
_SRC_SUBVOL = layer_resource_subvol(*_SRC_SUBVOL_PAIR)

_ATTACH_ANTLIR_DIR_CMD_ARGS = [
    "--attach-antlir-dir",
    "explicit_on",
    "--layer",
    layer_resource(*_SRC_SUBVOL_PAIR),
    "--",
    "test",
    "-d",
    ANTLIR_DIR,
]


class AttachAntlirDirTestCase(NspawnTestBase):
    def test_attach_antlir_dir(self):
        self.assertFalse(_SRC_SUBVOL.path(ANTLIR_DIR).exists())
        self._nspawn_in(
            _SRC_SUBVOL_PAIR,
            _ATTACH_ANTLIR_DIR_CMD_ARGS,
        )

    @with_temp_subvols
    def test_cleanup_antlir_dir(self, temp_subvols):
        dest_subvol = temp_subvols.external_command_will_create(
            "cleanup_antlir_dir"
        )
        self._nspawn_in(
            _SRC_SUBVOL_PAIR,
            [
                f"--snapshot-into={dest_subvol.path()}",
                *_ATTACH_ANTLIR_DIR_CMD_ARGS,
            ],
        )
        self.assertFalse(dest_subvol.path(ANTLIR_DIR).exists())
