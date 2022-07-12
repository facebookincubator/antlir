#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import sys
import textwrap
import unittest
from contextlib import contextmanager
from typing import Any, AnyStr, Iterable, Optional
from unittest.mock import patch

from antlir.bzl.genrule_layer import genrule_layer_t
from antlir.bzl.proxy_server_config import proxy_server_config_t
from antlir.bzl.target import target_t

from antlir.compiler.items.common import PhaseOrder
from antlir.compiler.items.genrule_layer import GenruleLayerItem
from antlir.compiler.items.make_subvol import ParentLayerItem
from antlir.compiler.items.tests.common import DUMMY_LAYER_OPTS
from antlir.config import antlir_dep
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import AttachAntlirDirMode
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource_subvol


def _touch_cmd(path: str):
    return ("/bin/touch", path)


def _item(
    cmd: Iterable[AnyStr],
    container_opts: Optional[Any] = None,
) -> GenruleLayerItem:
    if not container_opts:
        container_opts = genrule_layer_t.types.container_opts()
    return GenruleLayerItem(
        from_target="t",
        user="root",
        cmd=cmd,
        container_opts=container_opts,
    )


def _builder(
    cmd: Iterable[AnyStr],
    container_opts: Optional[Any] = None,
):
    return GenruleLayerItem.get_phase_builder(
        [_item(cmd=cmd, container_opts=container_opts)], DUMMY_LAYER_OPTS
    )


class GenruleLayerItemTestCase(unittest.TestCase):
    def test_phase_order(self):
        self.assertEqual(_item([]).phase_order(), PhaseOrder.GENRULE_LAYER)

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
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvols:
            # Cannot use `.snapshot()` since that doesn't handle mounts.
            child_sv = temp_subvols.caller_will_create(name)
            ParentLayerItem.get_phase_builder(
                [ParentLayerItem(from_target="t", subvol=parent_sv)],
                DUMMY_LAYER_OPTS,
            )(child_sv)
            yield child_sv

    def test_genrule_layer_basics(self):
        with self._temp_resource_subvol("genrule-layer-base") as subvol:
            _builder(_touch_cmd("/HELLO_ALIEN"))(subvol)

            alien_path = subvol.path("/HELLO_ALIEN")
            self.assertTrue(os.path.isfile(alien_path))
            alien_stat = os.stat(alien_path)
            self.assertEqual((0, 0), (alien_stat.st_uid, alien_stat.st_gid))

            self._check_protected_dir(subvol, "/.meta")
            self._check_protected_dir(subvol, "/__antlir__")

            snapshot_dir = snapshot_install_dir(
                antlir_dep("rpm:repo-snapshot-for-tests")
            )
            GenruleLayerItem.get_phase_builder(
                [
                    GenruleLayerItem(
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
                        container_opts=genrule_layer_t.types.container_opts(
                            serve_rpm_snapshots=[snapshot_dir]
                        ),
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
    def test_genrule_layer_no_antlir_dir(self):
        with self._temp_resource_subvol("genrule-layer-busybox-base") as sv:
            _builder(["/bin/sh", "-c", "echo ohai"])(sv)
            self.assertFalse(os.path.exists(sv.path("/__antlir__")))

    def test_genrule_layer_boot(self):
        with self._temp_resource_subvol("genrule-layer-base") as subvol:
            # this will fail if the genrule layer is not running in a booted
            # systemd container
            item = _item(["/bin/bash", "-c", "systemctl status > /status"])
            item = item.copy(update={"boot": True})
            GenruleLayerItem.get_phase_builder([item], DUMMY_LAYER_OPTS)(subvol)

    def test_genrule_proxy_server(self):
        # This test only ensures proper calls to nspawn plugins.
        # @oss-disable: # Full integration tests are done in           
        # @oss-disable: # antlir/bzl/genrule/facebook/chef_solo/tests  
        # @oss-disable: # and antlir/proxy/facebook/tests              
        fbpkg_db_path = target_t(name="fbpkg_db_path", path="path")

        with self._temp_resource_subvol(
            "genrule-layer-busybox-base"
        ) as sv, patch(
            "antlir.compiler.items.genrule_layer.run_nspawn"
        ) as run_nspawn, patch(
            "antlir.compiler.items.genrule_layer.NspawnPluginArgs"
        ) as plugin_args:
            _builder(
                ["/bin/sh", "-c", "echo ohai"],
                container_opts=genrule_layer_t.types.container_opts(
                    proxy_server_config=proxy_server_config_t(
                        fbpkg_db_path=fbpkg_db_path
                    )
                ),
            )(sv)
            run_nspawn.assert_called_once()
            plugin_args.assert_called_once_with(
                serve_rpm_snapshots=(),
                shadow_proxied_binaries=False,
                shadow_paths=(),
                proxy_server_config=proxy_server_config_t(
                    fbpkg_db_path=fbpkg_db_path
                ),
                attach_antlir_dir=(AttachAntlirDirMode.OFF),
            )
