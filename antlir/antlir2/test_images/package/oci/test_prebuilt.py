# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import filecmp
import json
import os
import subprocess
from pathlib import Path
from unittest import TestCase

from antlir.antlir2.test_images.package.oci.podman_helpers import load_image

OCI_FROM_PREBUILT: Path = Path(os.environ["OCI_FROM_PREBUILT"])
OCI_ORIGINAL: Path = Path(os.environ["OCI_ORIGINAL"])


def _load_manifest(oci_dir: Path) -> dict:
    index = json.loads((oci_dir / "index.json").read_text())
    manifest_digest = index["manifests"][0]["digest"].removeprefix("sha256:")
    return json.loads((oci_dir / "blobs" / "sha256" / manifest_digest).read_text())


def _load_config(oci_dir: Path) -> dict:
    manifest = _load_manifest(oci_dir)
    config_digest = manifest["config"]["digest"].removeprefix("sha256:")
    return json.loads((oci_dir / "blobs" / "sha256" / config_digest).read_text())


class PrebuiltTest(TestCase):
    def test_prebuilt_loads(self) -> None:
        self.assertIsNotNone(load_image(OCI_FROM_PREBUILT))

    def test_prebuilt_runs(self) -> None:
        image_id = load_image(OCI_FROM_PREBUILT)
        proc = subprocess.run(
            [
                "podman",
                "run",
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

    def test_prebuilt_has_new_content(self) -> None:
        image_id = load_image(OCI_FROM_PREBUILT)
        proc = subprocess.run(
            [
                "podman",
                "run",
                "--network=none",
                "--cgroups=disabled",
                "--entrypoint",
                "/bin/cat",
                image_id,
                "/prebuilt-test-marker",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual("roundtrip-success\n", proc.stdout)

    def test_prebuilt_has_labels(self) -> None:
        image_id = load_image(OCI_FROM_PREBUILT)
        proc = subprocess.run(
            ["podman", "inspect", image_id, "--format", "{{json .Config.Labels}}"],
            check=True,
            text=True,
            capture_output=True,
        )
        labels = json.loads(proc.stdout.strip())
        self.assertIsNotNone(labels)
        self.assertIn("com.meta.test.label", labels)
        self.assertEqual(labels["com.meta.test.label"], "test-value")
        self.assertIn("com.meta.test.another", labels)
        self.assertEqual(labels["com.meta.test.another"], "another-value")

    def test_prebuilt_has_env(self) -> None:
        image_id = load_image(OCI_FROM_PREBUILT)
        proc = subprocess.run(
            ["podman", "inspect", image_id, "--format", "{{json .Config.Env}}"],
            check=True,
            text=True,
            capture_output=True,
        )
        env_list = json.loads(proc.stdout.strip())
        self.assertIsNotNone(env_list)
        self.assertIn("HHVM_DISABLE_PERSONALITY=1", env_list)
        self.assertIn("TEST_ENV_VAR=test-value", env_list)

    def test_base_layer_blobs_preserved_bitwise(self) -> None:
        original_manifest = _load_manifest(OCI_ORIGINAL)
        rebuilt_manifest = _load_manifest(OCI_FROM_PREBUILT)

        original_layers = original_manifest["layers"]
        rebuilt_layers = rebuilt_manifest["layers"]
        self.assertGreater(
            len(rebuilt_layers),
            len(original_layers),
            "rebuilt image should have more layers than the original (base + addon)",
        )

        for i, original_layer in enumerate(original_layers):
            rebuilt_layer = rebuilt_layers[i]
            self.assertEqual(
                original_layer["digest"],
                rebuilt_layer["digest"],
                f"layer {i} digest mismatch: base layer descriptors should be an exact prefix",
            )
            self.assertEqual(
                original_layer["size"],
                rebuilt_layer["size"],
                f"layer {i} size mismatch",
            )

            original_blob = (
                OCI_ORIGINAL
                / "blobs"
                / "sha256"
                / original_layer["digest"].removeprefix("sha256:")
            )
            rebuilt_blob = (
                OCI_FROM_PREBUILT
                / "blobs"
                / "sha256"
                / rebuilt_layer["digest"].removeprefix("sha256:")
            )
            self.assertTrue(
                filecmp.cmp(original_blob, rebuilt_blob, shallow=False),
                f"layer {i} blob content differs: {original_blob} vs {rebuilt_blob}",
            )

    def test_base_history_preserved(self) -> None:
        original_config = _load_config(OCI_ORIGINAL)
        rebuilt_config = _load_config(OCI_FROM_PREBUILT)

        original_history = original_config.get("history", [])
        rebuilt_history = rebuilt_config.get("history", [])

        self.assertGreater(
            len(rebuilt_history),
            len(original_history),
            "rebuilt image should have more history entries than the original",
        )

        for i, original_entry in enumerate(original_history):
            self.assertEqual(
                original_entry,
                rebuilt_history[i],
                f"history entry {i} mismatch: base history should be an exact prefix",
            )
