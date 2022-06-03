# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import stat
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


class PackageImageTestCase(ImagePackageTestCaseBase):
    def _sibling_path(self, rel_path: str) -> Path:
        """Override ImagePackageTestCaseBase._sibling_path()."""
        return Path(__file__).dirname() / rel_path

    @contextmanager
    def _mount(self, image_path: Path) -> Path:
        with temp_dir() as mount_dir:
            subprocess.check_call(
                [
                    "mount",
                    "-t",
                    "btrfs",
                    "-o",
                    "loop,discard,nobarrier",
                    image_path,
                    mount_dir,
                ]
            )
            yield mount_dir

            subprocess.check_call(
                [
                    "umount",
                    mount_dir,
                ]
            )

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

    @classmethod
    def setUpClass(cls) -> None:
        os.mknod(
            "/dev/loop-control",
            mode=stat.S_IFCHR | 0o660,
            device=os.makedev(10, 237),
        )
        for i in range(63):
            os.mknod(
                f"/dev/loop{i}",
                mode=stat.S_IFBLK | 0o666,
                device=os.makedev(7, i),
            )

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
        ) as out_path, self._mount(
            image_path=out_path
        ) as mount_dir, tempfile.NamedTemporaryFile() as temp_sendstream:
            self.assertEqual(
                subprocess.check_output(
                    [
                        "findmnt",
                        "--mountpoint",
                        mount_dir,
                        "--noheadings",
                        "-o",
                        "LABEL",
                    ]
                )
                .decode()
                .strip(),
                "test-label",
            )

            subprocess.check_call(
                [
                    "btrfs",
                    "send",
                    "-f",
                    temp_sendstream.name,
                    mount_dir / "create_ops",
                ]
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

        # Test with a mocked estimated size that is much larger than the
        # actual subvol so that we can force minimizing the final volume
        with temp_dir() as td:
            out_path = td / "image.btrs"
            subvol = layer_resource_subvol(
                __package__, "build-appliance-testing"
            )

            subvol.estimate_content_bytes = unittest.mock.MagicMock(
                return_value=2048 * MiB
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

            # Confirm that the image got downsized
            self.assertLess(
                os.stat(out_path).st_size,
                2048 * MiB,
            )

    def test_package_btrfs_default_size(self):
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
            ),
        ) as out_path:
            test_size = os.stat(out_path).st_size

        # Verify the size of the package created via the bzl
        self.assertEqual(
            os.stat(
                layer_resource(__package__, "create_ops_btrfs"),
            ).st_size,
            test_size,
        )

    def test_package_image_as_btrfs_free_mb(self):
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
            ),
        ) as out_path:
            baseline_size = os.stat(out_path).st_size

        free_mb_tgt = 1024
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
                free_mb=free_mb_tgt,
            ),
        ) as out_path:
            test_size = os.stat(out_path).st_size

        # Check the rough size (within 100 Mb)
        free_bytes = test_size - baseline_size
        free_bytes_tgt = free_mb_tgt * MiB
        self.assertLessEqual(baseline_size, test_size)
        self.assertLessEqual(
            abs(free_bytes - free_bytes_tgt),
            100 * MiB,
            msg=(
                f"free_bytes is not within 100 MiB of target: {free_bytes_tgt}"
                + f", baseline_size: {baseline_size}"
                + f", test_size: {test_size}"
                + f", free_bytes: {free_bytes}"
            ),
        )

        # Verify the size of the package created via the bzl
        self.assertEqual(
            os.stat(
                layer_resource(__package__, "create_ops_free_mb_btrfs")
            ).st_size,
            test_size,
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
        ) as out_path, self._mount(image_path=out_path) as mount_dir:
            subprocess.check_call(
                [
                    "touch",
                    mount_dir / "volume" / "foo",
                ]
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
        ) as out_path, self._mount(
            image_path=out_path,
        ) as mount_dir:
            # The output of this command looks something like:
            #
            # b'ID 256 gen 9 top level 5 path create_ops\n'
            #
            # The last element is the name of the subvol.
            default_subvol_name = (
                subprocess.run(
                    [
                        "btrfs",
                        "subvolume",
                        "get-default",
                        mount_dir,
                    ],
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
        ) as out_path, self._mount(
            image_path=out_path,
        ) as mount_dir:
            # List all the subvols in the loopback
            subvol_list = [
                line.split(b" ")[-1]
                for line in subprocess.run(
                    [
                        "btrfs",
                        "subvolume",
                        "list",
                        mount_dir,
                    ],
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
        ) as out_path, self._mount(
            image_path=out_path,
        ) as mount_dir:
            subvol_list = [
                line.split(b" ")[-1]
                for line in subprocess.run(
                    [
                        "btrfs",
                        "subvolume",
                        "list",
                        mount_dir,
                    ],
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

    def test_package_image_bad_params(self):
        with self.assertRaisesRegex(
            UserError,
            (
                "The 'loopback_opts.size_mb' parameter is not supported for "
                "btrfs packages. Use 'free_mb' instead."
            ),
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
                    loopback_opts=loopback_opts_t(size_mb=255),
                ),
            ):
                pass
