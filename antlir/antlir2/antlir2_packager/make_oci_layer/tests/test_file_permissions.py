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


class TestFilePermissions(TestCase):
    """Test that file permissions are correctly preserved in OCI layer tars.

    When files have metadata-only changes (e.g., xattr modifications) without
    content changes, their filesystem permissions must be preserved in the
    generated OCI layer tar archive. This validates the fix in D88790669.
    """

    def load_image(self) -> str:
        """Load a docker archive into podman and return the image ID."""
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

    def test_file_permissions_preserved_with_metadata_change(self) -> None:
        """
        Verify that file permissions are preserved when only metadata changes.

        Layer structure:
        - Parent layer: Creates /test_file.sh with 0755 permissions
        - Child layer: Adds xattr to /test_file.sh (metadata-only change)

        Expected: /test_file.sh retains 0755 permissions in final image
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
                "/test_file.sh",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "755\n",
            proc.stdout,
            "File /test_file.sh should have 755 permissions",
        )

    def test_readonly_file_permissions_preserved(self) -> None:
        """
        Verify that readonly file permissions are preserved.

        Expected: /readonly.txt has 0444 permissions in final image
        """
        image_id = self.load_image()

        # Verify readonly file permissions
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
                "/readonly.txt",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "444\n",
            proc.stdout,
            "File /readonly.txt should have 444 permissions",
        )

    def test_private_file_permissions_preserved(self) -> None:
        """
        Verify that private file permissions are preserved.

        Expected: /private.txt has 0600 permissions in final image
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
                "/private.txt",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "600\n",
            proc.stdout,
            "File /private.txt should have 600 permissions",
        )
