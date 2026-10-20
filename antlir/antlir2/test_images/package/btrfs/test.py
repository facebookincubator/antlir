# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

from antlir.artifacts_dir import find_repo_root  # @dep=//antlir:fs_utils


class Test(unittest.TestCase):
    def test_label(self) -> None:
        for image in ("SIMPLE", "CAD_STACK", "CAD_STACK_REMOTE"):
            with self.subTest(image=image):
                info = subprocess.run(
                    ["btrfs", "filesystem", "label", os.environ[image]],
                    text=True,
                    capture_output=True,
                    check=True,
                )
                self.assertEqual(info.stdout.strip(), "mylabel")

    def test_materialization_cleanup(self) -> None:
        with open(os.environ["CAD_STACK_SPEC"]) as f:
            spec = json.load(f)
        btrfs = spec["btrfs"]
        repo_root = Path(os.fsdecode(find_repo_root()))
        btrfs["build_appliance"] = str(repo_root / btrfs["build_appliance"])
        for subvol in btrfs["subvols"].values():
            subvol["layers"] = [str(repo_root / layer) for layer in subvol["layers"]]
        layers = btrfs["subvols"]["/foo/baz/qux"]["layers"]
        packager = str(Path(os.environ["PACKAGER"]).absolute())
        with tempfile.TemporaryDirectory() as tmp:
            scratch = Path(tmp)
            materialized = scratch / "materialized"
            materialized.mkdir()
            for fail in (False, True):
                with self.subTest(fail=fail):
                    btrfs["subvols"]["/foo/baz/qux"]["layers"] = [] if fail else layers
                    spec_path = scratch / "spec.json"
                    spec_path.write_text(json.dumps(spec))
                    proc = subprocess.run(
                        [
                            packager,
                            f"--spec={spec_path}",
                            "--working-format=cad-stack",
                            "--rootless",
                            f"--out={scratch / 'image.btrfs'}",
                        ],
                        cwd=scratch,
                        env=os.environ | {"TMPDIR": str(materialized)},
                        capture_output=True,
                        text=True,
                    )
                    self.assertEqual(proc.returncode == 0, not fail, proc.stderr)
                    self.assertEqual(list(materialized.iterdir()), [])
                    self.assertFalse((scratch / "antlir2-out").exists())

    def test_hoist_parity(self) -> None:
        with open(os.environ["HOIST"], "rb") as f:
            expected = f.read()
        with open(os.environ["HOIST_CAD_STACK"], "rb") as f:
            actual = f.read()
        self.assertEqual(actual, expected)
        self.assertEqual(
            os.stat(os.environ["HOIST_CAD_STACK"]).st_mode & 0o7777,
            os.stat(os.environ["HOIST"]).st_mode & 0o7777,
        )

    def test_seed(self) -> None:
        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", os.environ["SEED"]],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("SEEDING", proc.stdout)

        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", os.environ["SIMPLE"]],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertNotIn("SEEDING", proc.stdout)

    def test_free_space(self) -> None:
        proc = subprocess.run(
            ["btrfs", "filesystem", "show", os.environ["FREE_SPACE"], "--raw"],
            check=True,
            capture_output=True,
            text=True,
        )

        match = re.search(r"\bsize\s+(\d+)\s+used\s+(\d+)\b", proc.stdout)
        self.assertIsNotNone(match, f"'{proc.stdout}' did not match")
        size = int(match.group(1))
        used = int(match.group(2))
        # The actual free space (size-used) should be at least the amount of
        # space that was requested
        self.assertGreaterEqual(size - used, int(os.environ["FREE_MB"]) * 1024 * 1024)

    def test_size(self) -> None:
        size_bytes = int(os.environ["SIZE_MB"]) * 1024 * 1024
        self.assertEqual(os.path.getsize(os.environ["SIZED"]), size_bytes)
        proc = subprocess.run(
            ["btrfs", "filesystem", "show", os.environ["SIZED"], "--raw"],
            check=True,
            capture_output=True,
            text=True,
        )
        match = re.search(r"\bsize\s+(\d+)\s+used\s+(\d+)\b", proc.stdout)
        self.assertIsNotNone(match, f"{proc.stdout!r} did not match")
        self.assertEqual(int(match.group(1)), size_bytes)
