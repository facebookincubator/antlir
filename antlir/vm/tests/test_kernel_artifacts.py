#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os

from antlir.tests.common import AntlirTestCase
from antlir.vm.kernel_artifacts_t import kernel_artifacts_t


class TestKernelArtifactsTest(AntlirTestCase):
    def test_load(self):
        kernel_artifacts = kernel_artifacts_t.read_resource(
            __package__, "kernel-artifacts"
        )

        self.assertIsInstance(kernel_artifacts, kernel_artifacts_t)

        # Verify that we have what we expect in the file generated for each
        # artifact layer
        for name in ["devel", "modules", "vmlinuz"]:
            artifact = getattr(kernel_artifacts, name)
            with open(artifact.subvol.path("data"), "r") as f:
                self.assertEqual(f.read(), f"{name}\n")

            self.assertEqual(artifact.name, f"//antlir/vm/tests:0.01-{name}")
            self.assertTrue(os.path.exists(artifact.path))

        self.assertEqual(kernel_artifacts.uname, "0.01")
