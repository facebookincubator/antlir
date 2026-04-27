# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import re
import subprocess
from pathlib import Path


def load_image(oci_path: Path) -> str:
    proc = subprocess.run(
        ["podman", "load", "--input", oci_path],
        check=True,
        text=True,
        capture_output=True,
    )
    match = re.match(r"^Loaded image: sha256:([a-f0-9]+)$", proc.stdout, re.MULTILINE)
    if match is None:
        raise ValueError(
            f"Could not parse image ID from podman load output: {proc.stdout}"
        )
    return match.group(1)
