#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import json
import os
from pathlib import Path
from unittest import TestCase

from antlir.antlir2.test_images.package.oci.podman_helpers import load_image

OCI_PATH: Path = Path(os.environ["OCI"])
ZSTD_CHUNKED_MEDIA_TYPE = "application/vnd.oci.image.layer.v1.tar+zstd"
ZSTD_CHUNKED_ANNOTATIONS = (
    "io.github.containers.zstd-chunked.manifest-checksum",
    "io.github.containers.zstd-chunked.manifest-position",
    "io.github.containers.zstd-chunked.tarsplit-position",
)


class TestZstdChunked(TestCase):
    def manifest(self) -> object:
        index = json.loads((OCI_PATH / "index.json").read_text())
        self.assertEqual(1, len(index["manifests"]))
        digest = index["manifests"][0]["digest"]
        algorithm, hex_digest = digest.split(":", 1)
        self.assertEqual("sha256", algorithm)
        return json.loads((OCI_PATH / "blobs" / algorithm / hex_digest).read_text())

    def test_oci_layout_uses_zstd_chunked_layers(self) -> None:
        manifest = self.manifest()
        layers = manifest["layers"]
        self.assertGreater(len(layers), 0)
        for layer in layers:
            self.assertEqual(ZSTD_CHUNKED_MEDIA_TYPE, layer["mediaType"])
            annotations = layer.get("annotations")
            self.assertIsNotNone(annotations)
            for annotation in ZSTD_CHUNKED_ANNOTATIONS:
                self.assertIn(annotation, annotations)
                self.assertNotEqual("", annotations[annotation])

    def test_podman_load_accepts_zstd_chunked_oci(self) -> None:
        self.assertIsNotNone(load_image(OCI_PATH))
