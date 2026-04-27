# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from pathlib import Path
from unittest import TestCase

from antlir.antlir2.test_images.package.oci.podman_helpers import load_image


class PathFilteringTest(TestCase):
    def _path_exists(self, image_id: str, path: str) -> bool:
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--network=none",
                "--cgroups=disabled",
                "--entrypoint",
                "/bin/bash",
                image_id,
                "-c",
                f"test -e {path}",
            ],
            text=True,
            capture_output=True,
        )
        return proc.returncode == 0

    def test_strip_paths(self) -> None:
        oci_path = Path(os.environ["OCI_STRIP"])
        image_id = load_image(oci_path)

        self.assertTrue(
            self._path_exists(image_id, "/kept-file"),
            "/kept-file should exist",
        )
        self.assertTrue(
            self._path_exists(image_id, "/entrypoint.sh"),
            "/entrypoint.sh should exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/ignored-file"),
            "/ignored-file should not exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/ignored-dir"),
            "/ignored-dir should not exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/ignored-dir/nested"),
            "/ignored-dir/nested should not exist",
        )

    def test_strip_whiteout(self) -> None:
        oci_path = Path(os.environ["OCI_STRIP_WHITEOUT"])
        image_id = load_image(oci_path)

        self.assertTrue(
            self._path_exists(image_id, "/wh-test-dir"),
            "/wh-test-dir should exist",
        )
        self.assertTrue(
            self._path_exists(image_id, "/wh-test-dir/kept-file"),
            "/wh-test-dir/kept-file should exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/wh-test-dir/removed-file"),
            "/wh-test-dir/removed-file should not exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/wh-deleted-dir"),
            "/wh-deleted-dir should not exist",
        )

    def test_retain_paths(self) -> None:
        oci_path = Path(os.environ["OCI_RETAIN"])
        image_id = load_image(oci_path)

        self.assertTrue(
            self._path_exists(image_id, "/kept-file"),
            "/kept-file should exist",
        )
        self.assertTrue(
            self._path_exists(image_id, "/entrypoint.sh"),
            "/entrypoint.sh should exist",
        )
        self.assertFalse(
            self._path_exists(image_id, "/ignored-file"),
            "/ignored-file should not exist (not in retain_paths)",
        )
        self.assertFalse(
            self._path_exists(image_id, "/ignored-dir"),
            "/ignored-dir should not exist (not in retain_paths)",
        )
