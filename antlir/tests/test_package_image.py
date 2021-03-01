#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
import stat
import subprocess
import sys
import tempfile
import unittest
from contextlib import contextmanager
from typing import Iterator

from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_as_corrupted_by_gnu_tar,
)
from antlir.fs_utils import generate_work_dir, open_for_read_decompress
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol
from antlir.tests.subvol_helpers import pop_path, render_sendstream

from ..find_built_subvol import _get_subvolumes_dir
from ..package_image import Format, package_image
from ..unshare import Namespace, Unshare, nsenter_as_root


def _render_sendstream_path(path):
    if path.endswith(".zst"):
        data = subprocess.check_output(
            ["zstd", "--decompress", "--stdout", path]
        )
    else:
        with open(path, "rb") as infile:
            data = infile.read()
    return render_sendstream(data)


class PackageImageTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        # Works in @mode/opt since the files of interest are baked into the XAR
        self.my_dir = os.path.dirname(__file__)

    @contextmanager
    def _package_image(
        self,
        layer_path: str,
        format: str,
        writable_subvolume: bool = False,
        seed_device: bool = False,
    ) -> Iterator[str]:
        with tempfile.TemporaryDirectory() as td:
            out_path = os.path.join(td, format)
            package_image(
                [
                    "--build-appliance",
                    layer_resource(__package__, "build-appliance"),
                    "--subvolumes-dir",
                    _get_subvolumes_dir(),
                    "--layer-path",
                    layer_path,
                    "--format",
                    format,
                    "--output-path",
                    out_path,
                    *(["--writable-subvolume"] if writable_subvolume else []),
                    *(["--seed-device"] if seed_device else []),
                ]
            )
            yield out_path

    def _sibling_path(self, rel_path: str):
        return os.path.join(self.my_dir, rel_path)

    def _assert_sendstream_files_equal(self, path1: str, path2: str):
        self.assertEqual(
            _render_sendstream_path(path1), _render_sendstream_path(path2)
        )

    # This tests `image_package.bzl` by consuming its output.
    def test_packaged_sendstream_matches_original(self):
        self._assert_sendstream_files_equal(
            self._sibling_path("create_ops-original.sendstream"),
            self._sibling_path("create_ops.sendstream"),
        )

    def test_package_image_as_sendstream(self):
        for format in ["sendstream", "sendstream.zst"]:
            with self._package_image(
                self._sibling_path("create_ops.layer"), format
            ) as out_path:
                self._assert_sendstream_files_equal(
                    self._sibling_path("create_ops-original.sendstream"),
                    out_path,
                )

    def test_package_image_as_btrfs_loopback(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "btrfs"
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, tempfile.TemporaryDirectory() as mount_dir, tempfile.NamedTemporaryFile() as temp_sendstream:  # noqa: E501
            # Future: use a LoopbackMount object here once that's checked in.
            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "mount",
                    "-t",
                    "btrfs",
                    "-o",
                    "loop,discard,nobarrier",
                    out_path,
                    mount_dir,
                )
            )
            try:
                # Future: Once I have FD, this should become:
                # Subvol(
                #     os.path.join(mount_dir.fd_path(), 'create_ops'),
                #     already_exists=True,
                # ).mark_readonly_and_write_sendstream_to_file(temp_sendstream)
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "btrfs",
                        "send",
                        "-f",
                        temp_sendstream.name,
                        os.path.join(mount_dir, "create_ops"),
                    )
                )
                self._assert_sendstream_files_equal(
                    self._sibling_path("create_ops-original.sendstream"),
                    temp_sendstream.name,
                )
            finally:
                nsenter_as_root(unshare, "umount", mount_dir)

    def test_package_image_as_btrfs_loopback_writable(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"),
            "btrfs",
            writable_subvolume=True,
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, tempfile.TemporaryDirectory() as mount_dir:
            os.chmod(
                out_path,
                stat.S_IMODE(os.stat(out_path).st_mode)
                | (stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH),
            )
            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "mount",
                    "-t",
                    "btrfs",
                    "-o",
                    "loop,discard,nobarrier",
                    out_path,
                    mount_dir,
                )
            )
            try:
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "touch",
                        os.path.join(mount_dir, "create_ops", "foo"),
                    )
                )
            finally:
                nsenter_as_root(unshare, "umount", mount_dir)

    def test_package_image_as_btrfs_seed_device(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"),
            "btrfs",
            writable_subvolume=True,
            seed_device=True,
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE,
            )
            self.assertIn(b"SEEDING", proc.stdout)
        with self._package_image(
            self._sibling_path("create_ops.layer"),
            "btrfs",
            writable_subvolume=True,
            seed_device=False,
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE,
            )
            self.assertNotIn(b"SEEDING", proc.stdout)

    def test_package_image_as_tarball(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "tar.gz"
        ) as out_path:
            # `write_tarball_to_file` provides full coverage for the tar
            # functionality, so we just need to test the integration here.
            self.assertLess(
                10,  # There are more files than that in `create_ops`
                len(
                    subprocess.check_output(["tar", "tzf", out_path]).split(
                        b"\n"
                    )
                ),
            )

    def test_image_layer_composed_with_tarball_package(self):
        # check that an image layer composed out of a tar-packaged create_ops
        # layer is equivalent to the original create_ops layer.
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            demo_sv_name = "demo_sv"
            demo_sv = temp_subvolumes.caller_will_create(demo_sv_name)
            with open(
                self._sibling_path("create_ops.sendstream")
            ) as f, demo_sv.receive(f):
                pass

            demo_render = render_demo_as_corrupted_by_gnu_tar(
                create_ops=demo_sv_name
            )

            with self._package_image(
                self._sibling_path("create_ops-layer-via-tarball-package"),
                "sendstream",
            ) as out_path:
                rendered_tarball_image = _render_sendstream_path(out_path)
                # This is metadata generated during the buck image build process
                # and is not useful for purposes of comparing the subvolume
                # contents.  However, it's useful to verify that the meta dir
                # we popped is what we expect.
                self.assertEqual(
                    [
                        "(Dir)",
                        {
                            "private": [
                                "(Dir)",
                                {
                                    "opts": [
                                        "(Dir)",
                                        {
                                            "artifacts_may_require_repo": [
                                                "(File d2)"
                                            ]
                                        },
                                    ]
                                },
                            ]
                        },
                    ],
                    pop_path(rendered_tarball_image, ".meta"),
                )
                self.assertEqual(demo_render, rendered_tarball_image)

    def test_format_name_collision(self):
        with self.assertRaisesRegex(AssertionError, "share format_name"):

            class BadFormat(Format, format_name="sendstream"):
                pass

    def _verify_package_as_squashfs(self, pkg_path):
        with TempSubvolumes(
            sys.argv[0]
        ) as temp_subvolumes, tempfile.NamedTemporaryFile() as temp_sendstream:
            subvol = temp_subvolumes.create("subvol")
            with Unshare(
                [Namespace.MOUNT, Namespace.PID]
            ) as unshare, tempfile.TemporaryDirectory() as mount_dir:
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "mount",
                        "-t",
                        "squashfs",
                        "-o",
                        "loop",
                        pkg_path,
                        mount_dir,
                    )
                )
                # `unsquashfs` would have been cleaner than `mount` +
                # `rsync`, and faster too, but unfortunately it corrupts
                # device nodes as of v4.3.
                subprocess.check_call(
                    nsenter_as_root(
                        unshare,
                        "rsync",
                        "--archive",
                        "--hard-links",
                        "--sparse",
                        "--xattrs",
                        mount_dir + "/",
                        subvol.path(),
                    )
                )
            with subvol.mark_readonly_and_write_sendstream_to_file(
                temp_sendstream
            ):
                pass
            original_render = _render_sendstream_path(
                self._sibling_path("create_ops-original.sendstream")
            )
            # SquashFS does not preserve the original's cloned extents of
            # zeros, nor the zero-hole-zero patter.  In all cases, it
            # (efficiently) transmutes the whole file into 1 sparse hole.
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
            self.assertEqual(
                original_render, _render_sendstream_path(temp_sendstream.name)
            )

    def test_package_image_as_squashfs(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "squashfs"
        ) as pkg_path:
            self._verify_package_as_squashfs(pkg_path)

        self._verify_package_as_squashfs(
            self._sibling_path("create_ops_squashfs")
        )

    def _verify_package_as_cpio(self, pkg_path):
        with TempSubvolumes(
            sys.argv[0]
        ) as temp_subvolumes, tempfile.NamedTemporaryFile() as temp_sendstream:
            extract_sv = temp_subvolumes.create("extract")
            work_dir = generate_work_dir()

            opts = new_nspawn_opts(
                cmd=[
                    "/bin/bash",
                    "-c",
                    "set -ue -o pipefail;" f"pushd {work_dir} >/dev/null;"
                    # -S to properly handle sparse files on extract
                    "/bin/bsdtar --file - --extract -S;",
                ],
                layer=layer_resource_subvol(__package__, "build-appliance"),
                bindmount_rw=[(extract_sv.path(), work_dir)],
                user=pwd.getpwnam("root"),
                allow_mknod=True,  # cpio archives support device files
            )

            with open_for_read_decompress(pkg_path) as r:
                run_nspawn(opts, PopenArgs(stdin=r))

            with extract_sv.mark_readonly_and_write_sendstream_to_file(
                temp_sendstream
            ):
                pass

            original_render = _render_sendstream_path(
                self._sibling_path("create_ops-original.sendstream")
            )

            # CPIO does not preserve the original's cloned extents, there's
            # really not much point in validating these so we'll just
            # set them to what they should be.
            original_render[1]["56KB_nuls"] = ["(File h57344)"]
            original_render[1]["56KB_nuls_clone"] = ["(File h57344)"]
            original_render[1]["zeros_hole_zeros"] = ["(File h49152)"]

            # CPIO does not support xattrs
            original_render[1]["hello"][0] = "(Dir)"

            # CPIO does not support unix sockets
            original_render[1].pop("unix_sock")

            self.assertEqual(
                original_render, _render_sendstream_path(temp_sendstream.name)
            )

    def test_package_image_as_cpio(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "cpio.gz"
        ) as pkg_path:
            self._verify_package_as_cpio(pkg_path)

        # Verify the explicit format version from the bzl
        self._verify_package_as_cpio(self._sibling_path("create_ops_cpio_gz"))
