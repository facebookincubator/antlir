#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class ManyDisksVMTest(AntlirTestCase):
    def test_device_names(self) -> None:
        for dev in (
            "vda",
            "vdb",
            "vdc",
            "vde",
            "vdf",
            "vdg",
            "vdh",
            "vdi",
            "vdj",
            "vdk",
            "vdl",
            "vdm",
            "vdn",
            "vdo",
            "vdp",
            "vdq",
            "vdr",
            "vds",
            "vdt",
            "vdu",
            "vdv",
            "vdw",
            "vdx",
            "vdy",
            "vdz",
            "vdaa",
            "vdab",
            "vdac",
            "vdad",
            "vdae",
            "vdaf",
            "vdag",
            "vdah",
            "vdai",
            "vdaj",
            "vdak",
        ):
            self.assertTrue(Path(f"/dev/{dev}").exists())
