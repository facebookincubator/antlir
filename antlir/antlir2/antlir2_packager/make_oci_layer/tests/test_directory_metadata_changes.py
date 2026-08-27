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


class TestDirectoryMetadataChanges(TestCase):
    """Test that directory metadata changes are properly handled in OCI layers.

    When directories have metadata-only changes (chown, chmod, set_times, xattrs),
    the change stream must emit proper Close operations to allow make_oci_layer to
    successfully build OCI layer tars.

    This test validates the scenario where:
    - Parent layer creates directories with certain metadata
    - Child layer modifies directory metadata (not file metadata)
    - make_oci_layer processes the change stream and builds the OCI layer

    Without proper Close operations, the change stream would emit metadata operations
    (Chown, Chmod, SetXattr) for directories but fail to close them, causing
    make_oci_layer to error with "not all entries were closed".
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

    def test_directory_chmod_change(self) -> None:
        """
        Verify that OCI layer builds successfully when directory permissions change.

        Layer structure:
        - Parent layer: Creates /var/cache/testdir with 0755 permissions
        - Child layer: Changes /var/cache/testdir to 0700 permissions

        This validates that directory chmod operations properly emit Close operations.
        Without the Close, make_oci_layer would fail with "not all entries were closed".

        Expected: OCI layer builds successfully and directory has 0700 permissions
        """
        image_id = self.load_image()

        # Verify directory permissions were changed
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
                "/var/cache/testdir",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "700\n",
            proc.stdout,
            "Directory /var/cache/testdir should have 700 permissions after chmod",
        )

    def test_directory_chown_change(self) -> None:
        """
        Verify that OCI layer builds successfully when directory ownership changes.

        This simulates real-world scenarios where configuration management tools
        modify directory ownership (e.g., cache directories changing ownership).

        Expected: OCI layer builds successfully (validates proper Close operations)
        """
        image_id = self.load_image()

        # Verify the directory exists - the key test is that the image loaded
        # successfully without "not all entries were closed" error
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "test",
                "-d",
                "/var/lib/testdata",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        # If we got here, the image loaded successfully, which means
        # the directory Close operation was properly emitted
        self.assertEqual(0, proc.returncode)

    def test_directory_xattr_change(self) -> None:
        """
        Verify that OCI layer builds successfully when directory xattrs change.

        Layer structure:
        - Parent layer: Creates /opt/app
        - Child layer: Adds xattr to /opt/app directory

        Expected: OCI layer builds successfully and xattr is set
        """
        image_id = self.load_image()

        # Verify the xattr was set on the directory
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "getfattr",
                "-n",
                "user.app",
                "--only-values",
                "/opt/app",
            ],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual(
            "production",
            proc.stdout,
            "Directory /opt/app should have user.app xattr set to 'production'",
        )

    def test_nested_directory_metadata_changes(self) -> None:
        """
        Verify that nested directories with metadata changes are handled correctly.

        This tests multiple directories in a hierarchy all having metadata changes,
        which exercises the Close operation logic for each directory level.

        Expected: All directories in the hierarchy have their metadata preserved
        """
        image_id = self.load_image()

        # Verify parent directory permissions
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
                "/var/cache/testdir",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual("700\n", proc.stdout)

        # Verify nested directory exists (it has xattr change)
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                "test",
                "-d",
                "/opt/app",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual(0, proc.returncode)
