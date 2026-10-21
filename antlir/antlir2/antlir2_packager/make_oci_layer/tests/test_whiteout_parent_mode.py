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


class TestWhiteoutParentMode(TestCase):
    """A layer that only deletes a file must not change its parent directories.

    The parent layer makes /opt/shared 0777 and puts a file under
    /opt/shared/lib. The child layer removes that file, so its OCI layer holds
    only the whiteout marker. Without entries for the marker's parent
    directories, podman recreates them in that layer as root 0755 and hides
    the 0777 from the parent layer.
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
        image_id = re.match(
            r"^Loaded image: sha256:([a-f0-9]+)$", proc.stdout, re.MULTILINE
        )
        self.assertIsNotNone(image_id)
        assert image_id is not None
        return image_id.group(1)

    def run_in_image(
        self, image_id: str, *cmd: str
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "podman",
                "run",
                "--rm",
                "--network=none",
                "--cgroups=disabled",
                image_id,
                *cmd,
            ],
            text=True,
            capture_output=True,
        )

    def test_whiteout_keeps_parent_modes(self) -> None:
        image_id = self.load_image()

        proc = self.run_in_image(
            image_id, "stat", "--format=%a", "/opt/shared", "/opt/shared/lib"
        )
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual("777\n755\n", proc.stdout)

    def test_whiteout_removes_the_file(self) -> None:
        image_id = self.load_image()

        proc = self.run_in_image(image_id, "test", "-e", "/opt/shared/lib/big.debug")
        self.assertNotEqual(0, proc.returncode)
        proc = self.run_in_image(image_id, "test", "-f", "/opt/shared/lib/keep.so")
        self.assertEqual(0, proc.returncode, proc.stderr)
