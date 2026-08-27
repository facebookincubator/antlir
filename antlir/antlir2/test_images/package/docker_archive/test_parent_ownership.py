#!/usr/bin/env python3
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


class TestParentOwnership(TestCase):
    """Test parent directory ownership preservation across image layers in OCI archives."""

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

    def test_parent_directory_ownership(self) -> None:
        """
        Verify that parent directories with custom ownership from parent layers
        are correctly preserved in the final OCI image when child layers add
        subdirectories.

        Layer structure:
        - Parent layer: Creates /var/foouser with foouser:foogroup ownership
        - Child layer: Creates /var/foouser/subdir and /var/foouser/another

        Expected: /var/foouser retains foouser:foogroup ownership in final image
        """
        image_id = self.load_image()

        # Verify parent directory ownership
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "stat",
                "--format=%U:%G",
                "/var/foouser",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "foouser:foogroup\n",
            proc.stdout,
            "Parent directory /var/foouser should have foouser:foogroup ownership",
        )

        # Verify subdirectory ownership
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "stat",
                "--format=%U:%G",
                "/var/foouser/subdir",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "foouser:foogroup\n",
            proc.stdout,
            "Subdirectory should have foouser:foogroup ownership",
        )
