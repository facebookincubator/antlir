#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
import stat
import subprocess
import tempfile
import unittest.mock
from contextlib import contextmanager
from typing import Iterator

from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_as_corrupted_by_cpio,
    render_demo_as_corrupted_by_gnu_tar,
)
from antlir.fs_utils import (
    generate_work_dir,
    open_for_read_decompress,
    Path,
    temp_dir,
)
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.serialize_targets_and_outputs import make_target_path_map
from antlir.subvol_utils import get_subvolumes_dir, MiB, with_temp_subvols
from antlir.tests.image_package_testbase import ImagePackageTestCaseBase
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol

from ..bzl.loopback_opts import loopback_opts_t
from ..package_image import _Opts, Format, package_image
from ..unshare import Namespace, nsenter_as_root, Unshare


class PackageImageTestCase(ImagePackageTestCaseBase):
    @contextmanager
    def _package_image(
        self,
        layer_path: str,
        format: str,
        loopback_opts: loopback_opts_t = None,
    ) -> Iterator[str]:
        target_map = make_target_path_map(os.environ["target_map"].split())
        with temp_dir() as td:
            out_path = td / format
            targets_and_outputs = td / "t_and_o.json"
            with targets_and_outputs.open("w") as f:
                f.write(Path.json_dumps(target_map))

            package_image(
                [
                    "--subvolumes-dir",
                    get_subvolumes_dir(),
                    "--layer-path",
                    layer_path,
                    "--format",
                    format,
                    "--output-path",
                    out_path,
                    *(
                        ["--loopback-opts", loopback_opts.json()]
                        if loopback_opts
                        else []
                    ),
                    "--targets-and-outputs",
                    targets_and_outputs,
                ]
            )
            yield out_path

    def _assert_sendstream_files_equal(self, path1: Path, path2: Path):
        self._assert_meta_valid_and_sendstreams_equal(
            self._render_sendstream_path(path1),
            self._render_sendstream_path(path2),
        )

    # This tests `image/package/new.bzl` by consuming its output.
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

    @with_temp_subvols
    def test_image_layer_composed_with_cpio_package(self, temp_subvolumes):
        # check that an image layer composed out of a tar-packaged create_ops
        # layer is equivalent to the original create_ops layer.
        demo_sv_name = "demo_sv"
        demo_sv = temp_subvolumes.caller_will_create(demo_sv_name)
        with open(
            self._sibling_path("create_ops.sendstream")
        ) as f, demo_sv.receive(f):
            pass

        demo_render = render_demo_as_corrupted_by_cpio(create_ops=demo_sv_name)

        with self._package_image(
            self._sibling_path("create_ops-layer-via-cpio-package"),
            "sendstream",
        ) as out_path:
            rendered_cpio_image = self._render_sendstream_path(out_path)
            # This is metadata generated during the buck image build process
            # and is not useful for purposes of comparing the subvolume
            # contents.  However, it's useful to verify that the meta dir
            # we popped is what we expect.
            self._assert_meta_valid_and_sendstreams_equal(
                demo_render, rendered_cpio_image
            )

    @with_temp_subvols
    def test_image_layer_composed_with_tarball_package(self, temp_subvolumes):
        # check that an image layer composed out of a tar-packaged create_ops
        # layer is equivalent to the original create_ops layer.
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
            rendered_tarball_image = self._render_sendstream_path(out_path)
            # This is metadata generated during the buck image build process
            # and is not useful for purposes of comparing the subvolume
            # contents.  However, it's useful to verify that the meta dir
            # we popped is what we expect.
            self._assert_meta_valid_and_sendstreams_equal(
                demo_render, rendered_tarball_image
            )

    def test_format_name_collision(self):
        with self.assertRaisesRegex(AssertionError, "share format_name"):

            class BadFormat(Format, format_name="sendstream"):
                pass

    @with_temp_subvols
    def _verify_package_as_squashfs(self, temp_subvolumes, pkg_path):
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
                    "--acls",
                    "--xattrs",
                    f"{mount_dir}/",
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

            # SquashFS does not preserve the original's cloned extents of
            # zeros, nor the zero-hole-zero patter.  In all cases, it
            # (efficiently) transmutes the whole file into 1 sparse hole.
            self._assert_ignore_original_extents(original_render)

            # squashfs does not support ACLs
            original_render[1]["dir_with_acls"][0] = "(Dir)"

            self._assert_meta_valid_and_sendstreams_equal(
                original_render,
                self._render_sendstream_path(Path(temp_sendstream.name)),
            )

    def test_package_image_as_squashfs(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "squashfs"
        ) as pkg_path:
            self._verify_package_as_squashfs(pkg_path)

    @with_temp_subvols
    def _verify_package_as_cpio(self, temp_subvolumes, pkg_path):
        with tempfile.NamedTemporaryFile() as temp_sendstream:
            extract_sv = temp_subvolumes.create("extract")
            work_dir = generate_work_dir()

            opts = new_nspawn_opts(
                cmd=[
                    "/bin/bash",
                    "-c",
                    "set -ue -o pipefail;"
                    f"pushd {work_dir.decode()} >/dev/null;"
                    # -S to properly handle sparse files on extract
                    "/bin/bsdtar --file - --extract -S;",
                ],
                layer=layer_resource_subvol(
                    __package__, "build-appliance-testing"
                ),
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

            original_render = self._render_sendstream_path(
                self._sibling_path("create_ops-original.sendstream")
            )

            # CPIO does not preserve the original's cloned extents, there's
            # really not much point in validating these so we'll just
            # set them to what they should be.
            self._assert_ignore_original_extents(original_render)

            # CPIO does not support ACLs
            original_render[1]["dir_with_acls"][0] = "(Dir)"

            # CPIO does not support xattrs
            original_render[1]["hello"][0] = "(Dir)"

            # CPIO does not support unix sockets
            original_render[1].pop("unix_sock")

            self._assert_meta_valid_and_sendstreams_equal(
                original_render,
                self._render_sendstream_path(Path(temp_sendstream.name)),
            )

    def test_package_image_as_cpio(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"), "cpio.gz"
        ) as pkg_path:
            self._verify_package_as_cpio(pkg_path)

        # Verify the explicit format version from the bzl
        self._verify_package_as_cpio(self._sibling_path("create_ops.cpio.gz"))

    def _verify_package_as_vfat(self, pkg_path, label="", fat_size=0):
        with Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir:
            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "mount",
                    "-t",
                    "vfat",
                    "-o",
                    "loop",
                    pkg_path,
                    mount_dir,
                )
            )
            self._verify_vfat_mount(unshare, mount_dir, label)
        # mkfs.vfat auto-selects fat size, only assert if we force a value
        if fat_size:
            fat_info = subprocess.check_output(["file", "-b", pkg_path])
            self.assertIn(f"({fat_size} bit)", str(fat_info))

    def test_package_image_as_vfat(self):
        with self._package_image(
            self._sibling_path("vfat-test.layer"),
            format="vfat",
            loopback_opts=loopback_opts_t(
                size_mb=32,
            ),
        ) as pkg_path:
            self._verify_package_as_vfat(pkg_path)

        # Verify different fat size works
        with self._package_image(
            self._sibling_path("vfat-test.layer"),
            format="vfat",
            loopback_opts=loopback_opts_t(
                size_mb=32,
                fat_size=16,
            ),
        ) as pkg_path:
            self._verify_package_as_vfat(pkg_path, fat_size=16)

        # Verify the explicit format version from the bzl
        self._verify_package_as_vfat(
            self._sibling_path("vfat-test.vfat"), label="cats"
        )

        # Verify size_mb is required
        with self.assertRaisesRegex(ValueError, "size_mb is required"):
            with self._package_image(
                self._sibling_path("vfat-test.layer"), "vfat"
            ) as pkg_path:
                pass

    def _verify_package_as_ext3(self, pkg_path, label=""):
        with Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir:
            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "mount",
                    "-t",
                    "ext3",
                    "-o",
                    "loop",
                    pkg_path,
                    mount_dir,
                )
            )
            self._verify_ext3_mount(unshare, mount_dir, label)

    def test_package_image_as_ext3(self):
        with self._package_image(
            self._sibling_path("create_ops.layer"),
            format="ext3",
            loopback_opts=loopback_opts_t(
                size_mb=256,
            ),
        ) as pkg_path:
            self._verify_package_as_ext3(pkg_path)

        self._verify_package_as_ext3(
            self._sibling_path("create_ops_ext3"), label="cats"
        )
        # Verify size_mb is required
        with self.assertRaisesRegex(ValueError, "size_mb is required"):
            with self._package_image(
                self._sibling_path("create_ops.layer"), "ext3"
            ) as pkg_path:
                pass
