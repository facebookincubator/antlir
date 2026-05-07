# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import subprocess
from pathlib import Path
from unittest import TestCase

from antlir.antlir2.test_images.package.oci.podman_helpers import load_image

OCI_PATH: Path = Path(os.environ["OCI"])


class Test(TestCase):
    def test_podman_load(self) -> None:
        self.assertIsNotNone(load_image(OCI_PATH))

    def test_podman_run(self) -> None:
        image_id = load_image(OCI_PATH)
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
        self.assertEqual(
            "Entrypoint!\n555 0 0\nstat: cannot statx '/to-be-deleted': No such file or directory\nrecreated\n",
            proc.stdout,
        )

    def test_image_has_labels(self) -> None:
        """Verify that the image contains the expected labels."""
        image_id = load_image(OCI_PATH)

        # Inspect the image to get labels
        proc = subprocess.run(
            ["podman", "inspect", image_id, "--format", "{{json .Config.Labels}}"],
            check=True,
            text=True,
            capture_output=True,
        )

        labels = json.loads(proc.stdout.strip())
        self.assertIsNotNone(labels, "Image should have labels")
        self.assertIn("com.meta.test.label", labels)
        self.assertEqual(labels["com.meta.test.label"], "test-value")
        self.assertIn("com.meta.test.another", labels)
        self.assertEqual(labels["com.meta.test.another"], "another-value")

    def test_image_has_env(self) -> None:
        """Verify that the image contains the expected environment variables."""
        image_id = load_image(OCI_PATH)

        # Inspect the image to get env
        proc = subprocess.run(
            ["podman", "inspect", image_id, "--format", "{{json .Config.Env}}"],
            check=True,
            text=True,
            capture_output=True,
        )

        env_list = json.loads(proc.stdout.strip())
        self.assertIsNotNone(env_list, "Image should have environment variables")
        self.assertIn("HHVM_DISABLE_PERSONALITY=1", env_list)
        self.assertIn("TEST_ENV_VAR=test-value", env_list)

    def test_image_has_user(self) -> None:
        """Verify that the image contains the expected default user."""
        image_id = load_image(OCI_PATH)

        proc = subprocess.run(
            ["podman", "inspect", image_id, "--format", "{{json .Config.User}}"],
            check=True,
            text=True,
            capture_output=True,
        )

        self.assertEqual("root", json.loads(proc.stdout.strip()))
