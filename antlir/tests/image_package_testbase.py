#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import os
import subprocess
import tempfile
import unittest

from antlir.subvol_utils import with_temp_subvols
from antlir.tests.subvol_helpers import (
    get_meta_dir_contents,
    pop_path,
    render_sendstream,
    render_subvol,
)

from ..unshare import Unshare, nsenter_as_root


class ImagePackageTestCaseBase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        # Works in @mode/opt since the files of interest are baked into the XAR
        self.my_dir = os.path.dirname(__file__)

    def _sibling_path(self, rel_path: str):
        return os.path.join(self.my_dir, rel_path)

    def _render_sendstream_path(self, path):
        if path.endswith(".zst"):
            data = subprocess.check_output(
                ["zstd", "--decompress", "--stdout", path]
            )
        else:
            with open(path, "rb") as infile:
                data = infile.read()
        return render_sendstream(data)

    def _assert_filesystem_label(
        self, unshare: Unshare, mount_dir: str, label: str
    ):
        self.assertEqual(
            subprocess.check_output(
                nsenter_as_root(
                    unshare,
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <:
                    #  [str, bytes]]]` for 2nd param but got `str`.
                    "findmnt",
                    "--mountpoint",
                    mount_dir,
                    "--noheadings",
                    "-o",
                    "LABEL",
                )
            )
            .decode()
            .strip(),
            label,
        )

    def _assert_ignore_original_extents(self, original_render):
        """
        Some filesystem formats do not preserve the original's cloned extents
        of zeros, nor the zero-hole-zero patter.
        """
        self.assertEqual(
            original_render[1].pop("56KB_nuls"),
            [
                "(File d57344(create_ops@56KB_nuls_clone:0+49152@0/"
                + "create_ops@56KB_nuls_clone:49152+8192@49152))"
            ],
        )
        original_render[1]["56KB_nuls"] = ["(File h57344)"]
        self.assertEqual(
            original_render[1].pop("56KB_nuls_clone"),
            [
                "(File d57344(create_ops@56KB_nuls:0+49152@0/"
                + "create_ops@56KB_nuls:49152+8192@49152))"
            ],
        )
        original_render[1]["56KB_nuls_clone"] = ["(File h57344)"]
        self.assertEqual(
            original_render[1].pop("zeros_hole_zeros"),
            ["(File d16384h16384d16384)"],
        )
        original_render[1]["zeros_hole_zeros"] = ["(File h49152)"]

    def _assert_meta_valid_and_sendstreams_equal(self, expected_stream, stream):
        self.assertEqual(get_meta_dir_contents(), pop_path(stream, ".meta"))
        self.assertEqual(
            expected_stream,
            stream,
        )

    @with_temp_subvols
    def _verify_ext3_mount(self, temp_subvolumes, unshare, mount_dir, label):
        self._assert_filesystem_label(unshare, mount_dir, label)
        subvol = temp_subvolumes.create("subvol")
        subprocess.check_call(
            nsenter_as_root(
                unshare,
                "rsync",
                "--archive",
                "--hard-links",
                "--sparse",
                "--xattrs",
                "--acls",
                mount_dir + "/",
                subvol.path(),
            )
        )
        with tempfile.NamedTemporaryFile() as temp_sendstream:
            with subvol.mark_readonly_and_write_sendstream_to_file(
                temp_sendstream
            ):
                pass
            original_render = self._render_sendstream_path(
                self._sibling_path("create_ops-original.sendstream")
            )
            self._assert_ignore_original_extents(original_render)

            # lost+found is an ext3 thing
            original_render[1]["lost+found"] = ["(Dir m700)", {}]

            self._assert_meta_valid_and_sendstreams_equal(
                original_render,
                self._render_sendstream_path(temp_sendstream.name),
            )

    @with_temp_subvols
    def _verify_vfat_mount(self, temp_subvolumes, unshare, mount_dir, label):
        self._assert_filesystem_label(unshare, mount_dir, label)
        subvol = temp_subvolumes.create("subvol")
        subprocess.check_call(
            nsenter_as_root(
                unshare,
                "cp",
                "-a",
                mount_dir + "/.",
                subvol.path(),
            )
        )
        self.assertEqual(
            render_subvol(subvol),
            [
                "(Dir)",
                {
                    "EFI": [
                        "(Dir)",
                        {
                            "BOOT": [
                                "(Dir)",
                                {"shadow_me": ["(File m755 d9)"]},
                            ]
                        },
                    ]
                },
            ],
        )
