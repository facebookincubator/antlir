#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import subprocess
import tempfile

from antlir.fs_utils import Path
from antlir.subvol_utils import with_temp_subvols
from antlir.tests.common import AntlirTestCase
from antlir.tests.subvol_helpers import (
    get_meta_dir_contents,
    pop_path,
    render_sendstream,
    render_subvol,
    RenderedTree,
)

from antlir.unshare import nsenter_as_root, Unshare


class ImagePackageTestCaseBase(AntlirTestCase):
    def setUp(self) -> None:
        super().setUp()
        # Works in @mode/opt since the files of interest are baked into the XAR
        self.my_dir = Path(__file__).dirname()

    def _sibling_path(self, rel_path: str) -> Path:
        return self.my_dir / rel_path

    def _render_sendstream_path(self, path: Path) -> RenderedTree:
        if path.endswith(b".zst"):
            data = subprocess.check_output(["zstd", "--decompress", "--stdout", path])
        else:
            with open(path, "rb") as infile:
                data = infile.read()
        return render_sendstream(data)

    def _assert_filesystem_label(
        self, unshare: Unshare, mount_dir: Path, label: str
    ) -> None:
        self.assertEqual(
            subprocess.check_output(
                nsenter_as_root(
                    unshare,
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

    def _assert_ignore_original_extents(self, original_render) -> None:
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

    def _assert_meta_valid_and_sendstreams_equal(self, expected_stream, stream) -> None:
        real_meta_contents = pop_path(stream, ".meta")
        if "build" in real_meta_contents[1]:
            # Don't check "build" key because length of target is unknown
            del real_meta_contents[1]["build"]

        self.assertEqual(get_meta_dir_contents(), real_meta_contents)
        self.assertEqual(
            expected_stream,
            stream,
        )

    @with_temp_subvols
    def _verify_ext3_mount(
        self, temp_subvolumes, unshare, mount_dir, label: str
    ) -> None:
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
                f"{mount_dir}/",
                subvol.path(),
            )
        )
        with tempfile.NamedTemporaryFile() as temp_sendstream:
            subvol.mark_readonly_and_write_sendstream_to_file(temp_sendstream)
            original_render = self._render_sendstream_path(
                self._sibling_path("create_ops-original.sendstream")
            )
            self._assert_ignore_original_extents(original_render)

            # lost+found is an ext3 thing
            original_render[1]["lost+found"] = ["(Dir m700)", {}]

            self._assert_meta_valid_and_sendstreams_equal(
                original_render,
                self._render_sendstream_path(Path(temp_sendstream.name)),
            )

    @with_temp_subvols
    def _verify_vfat_mount(
        self, temp_subvolumes, unshare, mount_dir, label: str
    ) -> None:
        self._assert_filesystem_label(unshare, mount_dir, label)
        subvol = temp_subvolumes.create("subvol")
        subprocess.check_call(
            nsenter_as_root(
                unshare,
                "cp",
                "-a",
                mount_dir / ".",
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
