# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This test is intended to act as an integration test for the compiler.
See `test_compiler.py` for more granular compiler unit tests.
"""

import socket
import sys
import tempfile
import unittest

from antlir.bzl.constants import flavor_config_t
from antlir.fs_utils import RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR, Path
from antlir.subvol_utils import TempSubvolumes, _query_uuid, Subvol
from antlir.tests.flavor_helpers import render_flavor
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol
from antlir.tests.subvol_helpers import render_subvol

from .. import subvolume_on_disk as svod
from ..compiler import build_image, parse_args


class CompilerIntegrationTestCase(unittest.TestCase):
    def test_compile(self):
        with Path.resource(
            __package__, "compiler-binary", exe=True
        ) as binary, TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            flavor_config = flavor_config_t(
                name="antlir_test",
                build_appliance="build-appliance-testing",
                rpm_installer="dnf",
                rpm_repo_snapshot=RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
                / "dnf",
            )

            # We want the compiler to create a nested subvol so the outer test
            # subvol can still be cleaned up despite the newly created nested
            # subvol being set the read only at the end of `build_image`.
            sv = temp_subvolumes.create("test")
            subvolumes_dir = temp_subvolumes.temp_dir
            subvol_rel_path = b"test/nested"

            deps = {
                "build-appliance-testing": layer_resource(
                    __package__, "test-build-appliance"
                )
            }

            # We write out tf to the temp subvol dir because it provides
            # cleanup and it resides inside the bind mounted artifacts dir
            # making it readable by the compiler running inside the BA.
            with tempfile.NamedTemporaryFile("w+t", dir=subvolumes_dir) as tf:
                tf.write(Path.json_dumps(deps))
                tf.seek(0)
                argv = [
                    "--artifacts-may-require-repo",
                    "--subvolumes-dir",
                    subvolumes_dir,
                    "--subvolume-rel-path",
                    subvol_rel_path,
                    "--flavor-config",
                    flavor_config.json(),
                    "--compiler-binary",
                    binary,
                    "--child-layer-target",
                    "CHILD_TARGET",
                    "--targets-and-outputs",
                    tf.name,
                ]
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
                    },
                ],
                render_subvol(sv_nested),
            )
            self.assertEqual(
                svod.SubvolumeOnDisk(
                    **{
                        svod._BTRFS_UUID: _query_uuid(
                            sv_nested, sv_nested.path()
                        ),
                        svod._BTRFS_PARENT_UUID: None,
                        svod._HOSTNAME: socket.gethostname(),
                        svod._SUBVOLUMES_BASE_DIR: subvolumes_dir,
                        svod._SUBVOLUME_REL_PATH: subvol_rel_path,
                        svod._BUILD_APPLIANCE_PATH: layer_resource_subvol(
                            __package__, "test-build-appliance"
                        ).path(),
                    }
                ),
                res,
            )
