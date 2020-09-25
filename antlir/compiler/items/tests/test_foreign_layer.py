#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import sys
import textwrap
import unittest
from contextlib import contextmanager
from typing import AnyStr, Iterable

from antlir.fs_utils import Path
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder
from ..foreign_layer import ForeignLayerItem
from ..foreign_layer_t import foreign_layer_t
from ..make_subvol import ParentLayerItem
from .common import DUMMY_LAYER_OPTS


def _touch_cmd(path: str):
    return ("/bin/touch", path)


def _item(cmd: Iterable[AnyStr]) -> ForeignLayerItem:
    return ForeignLayerItem(
        from_target="t",
        user="root",
        cmd=cmd,
        # Fixme: D23887778 makes this better.
        container_opts=foreign_layer_t.__annotations__["container_opts"](),
    )


def _builder(cmd: Iterable[AnyStr]):
    return ForeignLayerItem.get_phase_builder([_item(cmd)], DUMMY_LAYER_OPTS)


class ForeignLayerItemTestCase(unittest.TestCase):
    def test_phase_order(self):
        self.assertEqual(_item([]).phase_order(), PhaseOrder.FOREIGN_LAYER)

    def _check_protected_dir(self, subvol, protected_dir):
        protected_dir = Path(protected_dir)
        write_to_protected = _builder(_touch_cmd(protected_dir / "ALIEN"))
        with self.assertRaises(subprocess.CalledProcessError):
            write_to_protected(subvol)
        self.assertTrue(os.path.isdir(subvol.path(protected_dir)))
        self.assertFalse(os.path.exists(subvol.path(protected_dir / "ALIEN")))

    @contextmanager
    def _temp_resource_subvol(self, name: str):
        parent_sv = layer_resource_subvol(__package__, name)
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            # Cannot use `.snapshot()` since that doesn't handle mounts.
            child_sv = temp_subvols.caller_will_create(name)
            ParentLayerItem.get_phase_builder(
                [ParentLayerItem(from_target="t", subvol=parent_sv)],
                DUMMY_LAYER_OPTS,
            )(child_sv)
            yield child_sv

    def test_foreign_layer_basics(self):
        with self._temp_resource_subvol("foreign-layer-base") as subvol:
            _builder(_touch_cmd("/HELLO_ALIEN"))(subvol)

            alien_path = subvol.path("/HELLO_ALIEN")
            self.assertTrue(os.path.isfile(alien_path))
            alien_stat = os.stat(alien_path)
            self.assertEqual((0, 0), (alien_stat.st_uid, alien_stat.st_gid))

            self._check_protected_dir(subvol, "/.meta")
            self._check_protected_dir(subvol, "/__antlir__")

            snapshot_dir = snapshot_install_dir(
                "//antlir/rpm:repo-snapshot-for-tests"
            )
            ForeignLayerItem.get_phase_builder(
                [
                    ForeignLayerItem(
                        from_target="t",
                        user="root",
                        cmd=[
                            "/bin/sh",
                            "-c",
                            textwrap.dedent(
                                f"""
                    mkdir -p /install-root/.meta
                    {snapshot_dir}/dnf/bin/dnf \\
                        --installroot=/install-root --assumeyes \\
                            install rpm-test-carrot
                """
                            ),
                        ],
                        # Fixme: D23887778 makes this better.
                        container_opts=foreign_layer_t.__annotations__[
                            "container_opts"
                        ](serve_rpm_snapshots=[snapshot_dir]),
                    )
                ],
                DUMMY_LAYER_OPTS,
            )(subvol)
            # Not doing a rendered subvol test because RPM installation
            # is covered in so many other places.
            self.assertEqual(
                [b"carrot.txt"], subvol.path("/install-root/rpm_test").listdir()
            )

    # Checks that __antlir__ proctection handles a non-existent dir
    def test_foreign_layer_no_antlir_dir(self):
        with self._temp_resource_subvol("foreign-layer-busybox-base") as sv:
            _builder(["/bin/sh", "-c", "echo ohai"])(sv)
            self.assertFalse(os.path.exists(sv.path("/__antlir__")))
