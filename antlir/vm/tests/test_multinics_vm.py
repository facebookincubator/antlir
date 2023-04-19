#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class MultiNicVMTest(AntlirTestCase):
    def test_eth2_exists(self) -> None:
        self.assertTrue(Path("/sys/class/net/eth2/address").exists())
