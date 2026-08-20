# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import subprocess
from pathlib import Path
from subprocess import CalledProcessError
from unittest import TestCase

DOCKER_ARCHIVE_PATH: Path = Path(os.environ["DOCKER_ARCHIVE"])

# Fixed mtime the packager stamps onto every archive entry for reproducibility.
# Keep in sync with FIXED_MTIME in
# antlir/antlir2/antlir2_packager/make_oci_layer/src/main.rs.
FIXED_MTIME = 1075852800


class Test(TestCase):
    def load_image(self) -> str:
        try:
            proc = subprocess.run(
                ["podman", "load", "--input", DOCKER_ARCHIVE_PATH],
                check=True,
                text=True,
                capture_output=True,
            )
        except CalledProcessError as e:
            self.fail(f"podman load failed ({e.returncode}): {e.stdout}\n{e.stderr}")
        self.assertIn("Loaded image", proc.stdout)
        image_id = re.match(
            r"^Loaded image: sha256:([a-f0-9]+)$", proc.stdout, re.MULTILINE
        )
        self.assertIsNotNone(image_id)
        image_id = image_id.group(1)
        self.assertIsNotNone(image_id)
        return image_id

    def test_podman_load(self) -> None:
        self.assertIsNotNone(self.load_image())

    def test_podman_run(self) -> None:
        image_id = self.load_image()
        proc = subprocess.run(
            [
                "podman",
                "run",
                # Disable some podman features that are not supported in the
                # container environment this test runs in
                # This is *not* a limitation of the produced image
                "--network=none",
                "--cgroups=disabled",
                image_id,
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual("Entrypoint!\n555 0 0\n", proc.stdout)

    def test_dnf_installed_timestamps(self) -> None:
        # Files installed via dnf must have their mtime normalized to the fixed
        # timestamp so that the docker archive is reproducible.
        image_id = self.load_image()
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--network=none",
                "--cgroups=disabled",
                "--entrypoint",
                "stat",
                image_id,
                "--format=%Y",
                "/usr/bin/bash",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual(f"{FIXED_MTIME}\n", proc.stdout)
