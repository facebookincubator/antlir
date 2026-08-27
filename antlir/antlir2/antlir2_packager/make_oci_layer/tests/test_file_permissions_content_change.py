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


class TestFilePermissionsContentChange(TestCase):
    """Test that file permissions are preserved when file content changes.

    When files have content modifications (not just metadata changes), their
    filesystem permissions must be preserved in the generated OCI layer tar.
    This tests the Contents::File code path in make_oci_layer.
    """

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

    def test_executable_permissions_preserved_with_content_change(self) -> None:
        """
        Verify that executable permissions are preserved when content changes.

        Layer structure:
        - Parent layer: Creates /executable.sh with 0755 permissions
        - Child layer: Modifies /executable.sh content (not just metadata)

        Expected: /executable.sh retains 0755 permissions in final image
        """
        image_id = self.load_image()

        # Verify file permissions are preserved (should be 755)
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "stat",
                "--format=%a",
                "/executable.sh",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "755\n",
            proc.stdout,
            "File /executable.sh should have 755 permissions after content change",
        )

        # Also verify the content was actually modified
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "cat",
                "/executable.sh",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertIn(
            "modified",
            proc.stdout,
            "File content should be modified",
        )

    def test_config_file_permissions_preserved_with_content_change(self) -> None:
        """
        Verify that config file permissions are preserved when content changes.

        Expected: /config.conf has 0644 permissions in final image
        """
        image_id = self.load_image()

        # Verify file permissions
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "stat",
                "--format=%a",
                "/config.conf",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "644\n",
            proc.stdout,
            "File /config.conf should have 644 permissions after content change",
        )

    def test_secret_file_permissions_preserved_with_content_change(self) -> None:
        """
        Verify that secret file permissions are preserved when content changes.

        Expected: /secret.key has 0600 permissions in final image
        """
        image_id = self.load_image()

        # Verify private file permissions
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "stat",
                "--format=%a",
                "/secret.key",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "600\n",
            proc.stdout,
            "File /secret.key should have 600 permissions after content change",
        )
