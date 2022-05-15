# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
import unittest.mock
from contextlib import contextmanager
from typing import Iterator

from antlir.bzl.image.package.btrfs import btrfs_opts_t, btrfs_subvol_t
from antlir.bzl.loopback_opts import loopback_opts_t
from antlir.bzl.target import target_t
from antlir.errors import UserError
from antlir.fs_utils import Path, temp_dir
from antlir.package.btrfs import _FoundSubvolOpts, BtrfsImage, package_btrfs
from antlir.subvol_utils import get_subvolumes_dir, MiB
from antlir.tests.image_package_testbase import ImagePackageTestCaseBase
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol
from antlir.unshare import Namespace, nsenter_as_root, Unshare


class PackageImageTestCase(ImagePackageTestCaseBase):
    @contextmanager
    def _package_image(
        self,
        opts: btrfs_opts_t = None,
    ) -> Iterator[str]:
        with temp_dir() as td:
            out_path = td / "image.btrfs"
            opts_path = td / "opts.json"
            with opts_path.open("w") as f:
                f.write(opts.json())

            package_btrfs(
                [
                    "--subvolumes-dir",
                    get_subvolumes_dir(),
                    "--output-path",
                    out_path,
                    "--opts",
                    opts_path,
                ]
            )
            yield out_path

    def test_package_btrfs_estimated_size(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    # Keep this subvol name the same as the original
                    # so that when we compare sendstreams later in the
                    # test we get what is expected.
                    "/create_ops": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                },
                loopback_opts=loopback_opts_t(
                    label="test-label",
                ),
            ),
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir, tempfile.NamedTemporaryFile() as temp_sendstream:  # noqa: E501

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

            self._assert_filesystem_label(
                unshare,
                mount_dir,
                "test-label",
            )

            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "btrfs",
                    "send",
                    "-f",
                    temp_sendstream.name,
                    mount_dir / "create_ops",
                )
            )

            self._assert_meta_valid_and_sendstreams_equal(
                self._render_sendstream_path(
                    layer_resource(
                        __package__, "create_ops-original.sendstream"
                    )
                ),
                self._render_sendstream_path(
                    Path(temp_sendstream.name),
                ),
            )

    def test_package_btrfs_fixed_size(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/volume": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                },
                loopback_opts=loopback_opts_t(
                    size_mb=225,
                ),
            ),
        ) as out_path:
            self.assertEqual(
                os.stat(out_path).st_size,
                225 * MiB,
            )

    def test_package_btrfs_writable(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/volume": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                        writable=True,
                    ),
                },
            ),
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir:

            # Verify that we can write to the subvol
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

            subprocess.check_call(
                nsenter_as_root(
                    unshare,
                    "touch",
                    mount_dir / "volume" / "foo",
                )
            )

    def test_package_btrfs_seed_device(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/volume": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                        writable=True,
                    ),
                },
                seed_device=True,
            ),
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE,
            )
            self.assertIn(b"SEEDING", proc.stdout)

        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/volume": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                        writable=True,
                    ),
                },
                seed_device=False,
            ),
        ) as out_path:
            proc = subprocess.run(
                ["btrfs", "inspect-internal", "dump-super", out_path],
                check=True,
                stdout=subprocess.PIPE,
            )
            self.assertNotIn(b"SEEDING", proc.stdout)

    def test_package_btrfs_default_subvol(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/volume": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                },
                default_subvol=Path("/volume"),
            ),
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, tempfile.TemporaryDirectory() as mount_dir:
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

            # The output of this command looks something like:
            #
            # b'ID 256 gen 9 top level 5 path create_ops\n'
            #
            # The last element is the name of the subvol.
            default_subvol_name = (
                subprocess.run(
                    nsenter_as_root(
                        unshare,
                        "btrfs",
                        "subvolume",
                        "get-default",
                        mount_dir,
                    ),
                    check=True,
                    stdout=subprocess.PIPE,
                )
                .stdout.strip(b"\n")
                .split(b" ")[-1]
            )

            self.assertEqual(b"volume", default_subvol_name)

        # Test failure case with invalid default subvol
        with self.assertRaisesRegex(
            UserError,
            "AntlirUserError: Requested default: '/doesnotexist' is not a "
            "subvol being packaged:",
        ):
            with self._package_image(
                opts=btrfs_opts_t(
                    subvols={
                        "/volume": btrfs_subvol_t(
                            layer=target_t(
                                name="",
                                path=layer_resource(__package__, "create_ops"),
                            ),
                        ),
                    },
                    default_subvol=Path("/doesnotexist"),
                ),
            ) as out_path:
                pass

    def test_package_btrfs_too_small(self):
        with self.assertRaisesRegex(
            UserError,
            r"AntlirUserError: Unable to package subvol of \d+ bytes "
            r"into requested loopback size of \d+ bytes",
        ):
            with self._package_image(
                opts=btrfs_opts_t(
                    subvols={
                        "/volume": btrfs_subvol_t(
                            layer=target_t(
                                name="",
                                path=layer_resource(
                                    __package__, "build-appliance-testing"
                                ),
                            ),
                        ),
                    },
                    loopback_opts=loopback_opts_t(
                        # This is too small for the testing layer, which is
                        # a full OS image
                        size_mb=255
                    ),
                )
            ):
                pass

        # Test with a mocked estimated size so that we can force trying to
        # receive the subvol into a loopback that is intentionally sized
        # too small.
        with self.assertRaisesRegex(
            UserError,
            r"AntlirUserError: Receive failed. Subvol of \d+ bytes "
            r"did not fit into loopback of \d+ bytes",
        ):
            with temp_dir() as td:
                out_path = td / "image.btrs"
                subvol = layer_resource_subvol(
                    __package__, "build-appliance-testing"
                )

                # This size will force the receive itself to fail
                subvol.estimate_content_bytes = unittest.mock.MagicMock(
                    return_value=125 * MiB
                )

                subvols = {
                    Path("/volume"): _FoundSubvolOpts(
                        subvol=subvol,
                        writable=False,
                    )
                }

                BtrfsImage().package(
                    out_path,
                    subvols,
                )

    def test_package_btrfs_multiple_subvol(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/vol1": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                    "/vol2": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                },
            ),
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir:
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

            # List all the subvols in the loopback
            subvol_list = [
                line.split(b" ")[-1]
                for line in subprocess.run(
                    nsenter_as_root(
                        unshare,
                        "btrfs",
                        "subvolume",
                        "list",
                        mount_dir,
                    ),
                    check=True,
                    stdout=subprocess.PIPE,
                )
                .stdout.strip(b"\n")
                .split(b"\n")
            ]

            self.assertEqual(subvol_list, [b"vol1", b"vol2"])

    def test_package_btrfs_nested_subvol(self):
        with self._package_image(
            opts=btrfs_opts_t(
                subvols={
                    "/grand-parent": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                    "/grand-parent/parent/child": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                    "/grand-parent/parent": btrfs_subvol_t(
                        layer=target_t(
                            name="",
                            path=layer_resource(__package__, "create_ops"),
                        ),
                    ),
                },
            ),
        ) as out_path, Unshare(
            [Namespace.MOUNT, Namespace.PID]
        ) as unshare, temp_dir() as mount_dir:
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

            subvol_list = [
                line.split(b" ")[-1]
                for line in subprocess.run(
                    nsenter_as_root(
                        unshare,
                        "btrfs",
                        "subvolume",
                        "list",
                        mount_dir,
                    ),
                    check=True,
                    stdout=subprocess.PIPE,
                )
                .stdout.strip(b"\n")
                .split(b"\n")
            ]

            self.assertEqual(
                subvol_list,
                [
                    b"grand-parent",
                    b"grand-parent/parent",
                    b"grand-parent/parent/child",
                ],
            )

    def test_package_btrfs_sanity_check_subvols(self):
        # Verify subvol names start with /
        with self.assertRaisesRegex(
            UserError,
            "Requested subvol name must be an absolute path: volume",
        ):
            with self._package_image(
                opts=btrfs_opts_t(
                    subvols={
                        "volume": btrfs_subvol_t(
                            layer=target_t(
                                name="",
                                path=layer_resource(__package__, "create_ops"),
                            ),
                        ),
                    },
                ),
            ):
                pass

        # Verify default subvol starts with /
        with self.assertRaisesRegex(
            UserError, "Requested default: 'noslash' must be an absolute path."
        ):
            with self._package_image(
                opts=btrfs_opts_t(
                    subvols={
                        "/noslash": btrfs_subvol_t(
                            layer=target_t(
                                name="",
                                path=layer_resource(__package__, "create_ops"),
                            ),
                        ),
                    },
                    default_subvol="noslash",
                ),
            ):
                pass
