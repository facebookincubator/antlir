#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from contextlib import contextmanager

from antlir.fs_utils import temp_dir

from antlir.gpt import make_gpt
from antlir.tests.image_package_testbase import ImagePackageTestCaseBase
from antlir.tests.layer_resource import layer_resource
from antlir.unshare import Namespace, nsenter_as_root, Unshare


class GptTestCase(ImagePackageTestCaseBase):
    @contextmanager
    def _make_gpt(self):
        with temp_dir() as td:
            out_path = td / "image.gpt"
            make_gpt(
                [
                    "--build-appliance",
                    layer_resource(__package__, "build-appliance"),
                    "--output-path",
                    out_path,
                    "--gpt",
                    os.environ["test-gpt-json"],
                ]
            )
            yield out_path

    def _verify_gpt_image(self, image_path):
        with Unshare([Namespace.MOUNT, Namespace.PID]) as unshare:
            # verify name of second partition has been set to "create_ops_ext3"
            res = (
                subprocess.check_output(
                    nsenter_as_root(
                        unshare,
                        "partx",
                        "-n",
                        "2",
                        "-o",
                        "NAME",
                        "-g",
                        "--raw",
                        image_path,
                    )
                )
                .decode()
                .strip()
            )
            self.assertEqual(res, "create_ops_ext3")

            # verify that the third partition is a BIOS boot partition
            res = (
                subprocess.check_output(
                    nsenter_as_root(
                        unshare,
                        "partx",
                        "-n",
                        "3",
                        "-o",
                        "TYPE",
                        "-g",
                        "--raw",
                        image_path,
                    )
                )
                .decode()
                .strip()
            )
            self.assertEqual(res, "21686148-6449-6e6f-744e-656564454649")

            # verify partitiion contents
            res = (
                subprocess.check_output(
                    nsenter_as_root(
                        unshare,
                        "partx",
                        "-o",
                        "START,SECTORS",
                        "-g",
                        "--raw",
                        image_path,
                    )
                )
                .decode()
                .strip()
                .split("\n")
            )
            # partx offset units are in sectors
            sector_size = 512
            part_offsets = [int(o.split()[0]) * sector_size for o in res]
            part_sizes = [int(o.split()[1]) * sector_size for o in res]
            with temp_dir() as mount_dir:
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "mount",
                        "-t",
                        "vfat",
                        "-o",
                        "loop,"
                        f"offset={part_offsets[0]},"
                        f"sizelimit={part_sizes[0]}",
                        image_path,
                        mount_dir,
                    )
                )
                self._verify_vfat_mount(unshare, mount_dir, "cats")

            with temp_dir() as mount_dir:
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "mount",
                        "-t",
                        "ext3",
                        "-o",
                        f"loop,"
                        f"offset={part_offsets[1]},"
                        f"sizelimit={part_sizes[1]}",
                        image_path,
                        mount_dir,
                    )
                )
                self._verify_ext3_mount(unshare, mount_dir, "cats")

    def test_gpt_image(self):
        with self._make_gpt() as image_path:
            self._verify_gpt_image(image_path)
        self._verify_gpt_image(self._sibling_path("gpt_test"))
