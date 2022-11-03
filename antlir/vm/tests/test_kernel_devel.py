#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import platform

from antlir.tests.common import AntlirTestCase


class KernelDevelTest(AntlirTestCase):
    def test_vm_has_devel(self) -> None:
        uname = platform.release()

        self.assertTrue(os.path.ismount(os.path.join("/usr/src/kernels", uname)))
        self.assertTrue(
            os.path.ismount(os.path.join("/usr/lib/modules", uname, "build"))
        )
