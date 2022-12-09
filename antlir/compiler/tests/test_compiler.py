# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This test is intended to act as an integration test for the compiler.
See `test_compiler.py` for more granular compiler unit tests.
"""

import os
import socket
import sys
import tempfile
import unittest
from typing import List, Optional
from uuid import UUID

from antlir.btrfsutil import subvolume_info
from antlir.bzl.constants import flavor_config_t

from antlir.compiler import subvolume_on_disk as svod
from antlir.compiler.compiler import build_image, parse_args
from antlir.fs_utils import Path, RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR, temp_dir
from antlir.subvol_utils import Subvol, TempSubvolumes
from antlir.tests.flavor_helpers import render_flavor
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol
from antlir.tests.subvol_helpers import render_meta_build_contents, render_subvol

_TEST_BA_TARGET_PATH = "fbcode//antlir/compiler/test_images:build_appliance_testing"
_TEST_BA = layer_resource(__package__, "test-build-appliance")
_TEST_BA_SUBVOL = layer_resource_subvol(__package__, "test-build-appliance")


class CompilerIntegrationTestCase(unittest.TestCase):
    def compile(
        self,
        *,
        extra_args: Optional[List[str]] = None,
        include_flavor_config: bool = True,
        include_build_appliance: bool = True,
    ):
        extra_args = extra_args or []
        with TempSubvolumes() as temp_subvolumes:
            flavor_config = flavor_config_t(
                name="antlir_test",
                build_appliance=_TEST_BA_TARGET_PATH
                if include_build_appliance
                else None,
                rpm_installer="dnf",
                rpm_repo_snapshot=RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / "dnf",
            )

            # We want the compiler to create a nested subvol so the outer test
            # subvol can still be cleaned up despite the newly created nested
            # subvol being set the read only at the end of `build_image`.
            sv = temp_subvolumes.create("test")
            subvolumes_dir = temp_subvolumes.temp_dir
            subvol_rel_path = b"test/nested"

            # TODO(targets-and-outputs): create this at buck time, not here
            tao = {
                "metadata": {
                    "buck_version": 2,
                    "default_cell": "antlir",
                },
                "targets_and_outputs": {
                    _TEST_BA_TARGET_PATH: _TEST_BA,
                    "fbcode//antlir:empty": layer_resource(__package__, "empty"),
                },
            }

            # We write out tf to the temp subvol dir because it provides
            # cleanup and it resides inside the bind mounted artifacts dir
            # making it readable by the compiler running inside the BA.
            with tempfile.NamedTemporaryFile("w+t", dir=subvolumes_dir) as tf:
                tf.write(Path.json_dumps(tao))
                tf.seek(0)
                argv = [
                    "--artifacts-may-require-repo",
                    "--subvolumes-dir",
                    subvolumes_dir,
                    "--subvolume-rel-path",
                    subvol_rel_path,
                    *(
                        (
                            "--flavor-config",
                            flavor_config.json(),
                        )
                        if include_flavor_config
                        else ()
                    ),
                    "--compiler-binary",
                    os.environ["test_antlir_compiler_binary_path"],
                    "--child-layer-target",
                    "cell//some/child:target",
                    "--targets-and-outputs",
                    tf.name,
                    # We need to compile at least one feature to cover the code
                    # for compiler item building in `compile_items_to_subvol`.
                    "--child-feature-json",
                    os.environ["test_compiler_feature"],
                    *extra_args,
                ]
                print(argv, file=sys.stderr)
                res = build_image(parse_args(argv), argv)

            # `build_image` should have constructed `sv_nested`.
            sv_nested = Subvol(sv.path("nested"), already_exists=True)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        ".meta": [
                            "(Dir)",
                            {
                                "build": render_meta_build_contents(sv_nested),
                                "flavor": [render_flavor(flavor="antlir_test")],
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
                                ],
                            },
                        ],
                        "empty": ["(File m444)"],
                    },
                ],
                render_subvol(sv_nested),
            )
            self.assertEqual(
                svod.SubvolumeOnDisk(
                    **{
                        svod._BTRFS_UUID: str(
                            UUID(bytes=subvolume_info(sv_nested.path()).uuid)
                        ),
                        svod._BTRFS_PARENT_UUID: None,
                        svod._HOSTNAME: socket.gethostname(),
                        svod._SUBVOLUMES_BASE_DIR: subvolumes_dir,
                        svod._SUBVOLUME_REL_PATH: subvol_rel_path,
                        svod._BUILD_APPLIANCE_PATH: _TEST_BA_SUBVOL.path()
                        if include_build_appliance
                        else None,
                    }
                ),
                res,
            )

    def test_profiler(self):
        with temp_dir() as profile_dir:
            self.compile(extra_args=[f"--profile={profile_dir}"])
            # This profile won't actually be populated since the profiling and
            # pstat dump should occur outside `build_image`.
            self.assertTrue((profile_dir / "cell__some_child:target.pstat").exists())

    def test_no_flavor_config_or_parent_layer_error(self):
        with self.assertRaisesRegex(
            AssertionError,
            "Parent layer must be given if no flavor config is given",
        ):
            self.compile(include_flavor_config=False)

    def test_write_parent_flavor(self):
        self.compile(
            extra_args=[
                "--parent-layer",
                _TEST_BA,
            ],
            include_flavor_config=False,
        )

    def test_compiler_no_ba(self):
        # In the case where no BA is available, we still expect the compiler to
        # successfully compile by skipping the compiler re-invocation nspawn.
        self.compile(include_build_appliance=False)
