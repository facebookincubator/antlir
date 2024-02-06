# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import subprocess
import unittest

EFI_GUID = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
LINUX_FS_GUID = "0fc63daf-8483-4772-8e79-3d69d8477de4"


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.maxDiff = None

    def test_gpt_disk(self) -> None:
        info = subprocess.run(
            [
                "lsblk",
                "--output",
                "name,partlabel,size,parttype,fstype",
                "--json",
                "--bytes",
                "/dev/vdb",
            ],
            text=True,
            capture_output=True,
            check=True,
        )
        info = json.loads(info.stdout)
        self.assertEqual(info["blockdevices"][0]["name"], "vdb")
        partitions = info["blockdevices"][0]["children"]
        partitions = {p["name"]: p for p in partitions}
        # Assert the size of the ESP partition exactly since it's a critical api
        # feature to be able to set things like this exactly
        esp_size = partitions["vdb1"].pop("size")
        # 128MiB = 134,217,728. 134,217,728B + 512B = 134,218,240B
        self.assertEqual(esp_size, (int(os.environ["ESP_SIZE_MB"]) * 1024 * 1024) + 512)
        # the size of the btrfs partition isn't that interesting and may or may
        # not be stable
        del partitions["vdb2"]["size"]
        # Assert some other metadata: partition type, labels, etc
        self.assertEqual(
            partitions,
            {
                "vdb1": {
                    "name": "vdb1",
                    "partlabel": "ESP",
                    "parttype": EFI_GUID,
                    "fstype": "vfat",
                },
                "vdb2": {
                    "name": "vdb2",
                    "partlabel": None,
                    "parttype": LINUX_FS_GUID,
                    "fstype": "btrfs",
                },
            },
        )
