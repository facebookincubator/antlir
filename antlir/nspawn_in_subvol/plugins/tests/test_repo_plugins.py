#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import pwd
import unittest
from unittest import mock

from antlir.bzl.container_opts import shadow_path_t
from antlir.fs_utils import (
    ANTLIR_DIR,
    Path,
    RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR,
)
from antlir.nspawn_in_subvol.args import (
    AttachAntlirDirMode,
    new_nspawn_opts,
    NspawnPluginArgs,
)
from antlir.nspawn_in_subvol.common import AttachAntlirDirError

from antlir.nspawn_in_subvol.plugins import repo_plugins
from antlir.tests.flavor_helpers import get_rpm_installers_supported
from antlir.tests.layer_resource import layer_resource_subvol


def with_mock_plugins(method):
    @functools.wraps(method)
    def decorated(self, *args, **kwargs):
        with unittest.mock.patch.object(
            repo_plugins,
            "YumDnfVersionlock",
            side_effect=lambda x, y: ("fake_version_lock", x, y),
        ) as _, unittest.mock.patch.object(
            repo_plugins,
            "RepoServers",
            side_effect=lambda x, y, z: (
                "fake_repo_server",
                x,
            ),
        ) as _, unittest.mock.patch.object(
            repo_plugins,
            "ShadowPaths",
            side_effect=lambda x, y: (
                "fake_shadow_paths",
                x,
                set(y),
            ),
        ) as _, unittest.mock.patch.object(
            repo_plugins,
            "AttachAntlirDir",
            side_effect=lambda: ("fake_attach_antlir_dir"),
        ) as _:
            return method(self, *args, **kwargs)

    return decorated


class RpmPluginsTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def _create_snapshot_dir(self, attach_antlir_dir, svod=None):
        mock_path = mock.Mock()
        mock_path.exists = mock.Mock(return_value=not attach_antlir_dir)

        mock_subvol = mock.Mock()
        mock_subvol.path = mock.Mock(return_value=mock_path)

        return (
            repo_plugins._get_snapshot_dir(
                opts=new_nspawn_opts(
                    cmd=[], layer=mock_subvol, subvolume_on_disk=svod
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=False,
                    shadow_paths=[shadow_path_t(dst="src", src="dest")],
                    serve_rpm_snapshots=("a", "b", "c"),
                    snapshots_and_versionlocks=[("a", "vla"), ("c", "vlc")],
                    attach_antlir_dir=attach_antlir_dir,
                ),
            ),
            mock_subvol,
        )

    def test_get_snapshot_dir(self):
        snapshot_dir, mock_subvol = self._create_snapshot_dir(
            AttachAntlirDirMode.OFF
        )

        self.assertEqual(
            mock_subvol.path(RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR),
            snapshot_dir,
        )

    def test_get_snapshot_dir_and_attach_antlir_dir(self):
        build_appliance_path = Path("build_appliance_path")
        build_appliance_snapshot_dir = (
            build_appliance_path
            / RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR.strip_leading_slashes()
        )

        mock_svod = mock.Mock()
        mock_svod.build_appliance_path = build_appliance_path

        for attach_antlir_dir in [
            AttachAntlirDirMode.DEFAULT_ON,
            AttachAntlirDirMode.EXPLICIT_ON,
        ]:
            snapshot_dir, _ = self._create_snapshot_dir(
                attach_antlir_dir, svod=mock_svod
            )

            self.assertEqual(
                build_appliance_snapshot_dir,
                snapshot_dir,
            )

    def test_get_snapshot_dir_explicit_error(self):
        with self.assertRaisesRegex(
            AttachAntlirDirError,
            "Could not attach /__antlir__ dir",
        ):
            self._create_snapshot_dir(AttachAntlirDirMode.EXPLICIT_ON)

    def _check_repo_nspawn_plugins(
        self,
        attach_antlir_dir,
        shadow_proxied_binaries,
        mock_subvol,
        user,
    ):
        mock_path = mock.Mock()
        mock_path.exists = lambda: True
        build_appliance_path = mock.Mock()
        build_appliance_path.__truediv__ = lambda x, y: mock_path
        mock_svod = mock.Mock()
        mock_svod.build_appliance_path = build_appliance_path

        self.assertEqual(
            (
                *(
                    ("fake_attach_antlir_dir",)
                    if attach_antlir_dir != AttachAntlirDirMode.OFF
                    else ()
                ),
                (
                    "fake_shadow_paths",
                    [shadow_path_t(dst="src", src="dest")],
                    set(),
                ),
                (
                    "fake_version_lock",
                    [("a", "vla"), ("c", "vlc")],
                    {"a", "b", "c"},
                ),
                ("fake_repo_server", {"a", "b", "c"}),
            ),
            repo_plugins.repo_nspawn_plugins(
                opts=new_nspawn_opts(
                    cmd=[],
                    layer=mock_subvol,
                    subvolume_on_disk=mock_svod,
                    user=user,
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=shadow_proxied_binaries,
                    shadow_paths=[shadow_path_t(dst="src", src="dest")],
                    serve_rpm_snapshots=("a", "b", "c"),
                    snapshots_and_versionlocks=[("a", "vla"), ("c", "vlc")],
                    attach_antlir_dir=attach_antlir_dir,
                    proxy_server_config=None,
                ),
            ),
        )

    def _create_test_repo_nspawn_plugins_subvol(self, paths, paths_exist):
        mock_subvol = mock.Mock()
        mocks = {}
        for i, path in enumerate(paths):
            mock_path = mock.Mock()
            mock_path.exists = mock.Mock(return_value=paths_exist[i])
            mocks[path] = mock_path
        mock_subvol.path = mock.Mock(side_effect=lambda x: mocks[x])
        return mock_subvol

    # This fully mocked because we have lots of integration tests:
    #   - the per-plugin tests
    #   - `test-rpm-installer-shadow-paths`
    @with_mock_plugins
    def test_repo_nspawn_plugins(self):
        paths = [ANTLIR_DIR, RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR]

        # None of these will trigger automatic shadowing
        for (
            attach_antlir_dir,
            shadow_proxied_binaries,
            user,
            # The values in `paths_exist` and `path_check_count`
            # count correspond to the values in `path`,
            # and specify the return value of the `path.exists()`
            # as well as the number of times this method is called.
            paths_exist,
            paths_check_count,
        ) in [
            (
                AttachAntlirDirMode.OFF,
                False,
                pwd.getpwnam("root"),
                [False, True],
                [0, 0],
            ),  # disabled
            (
                AttachAntlirDirMode.OFF,
                True,
                pwd.getpwnam("nobody"),
                [False, True],
                [0, 0],
            ),  # not root
            (
                AttachAntlirDirMode.DEFAULT_ON,
                True,
                pwd.getpwnam("nobody"),
                [False, True],
                [2, 0],
            ),  # attach_antlir_dir
            (
                AttachAntlirDirMode.OFF,
                True,
                pwd.getpwnam("root"),
                [False, False],
                [0, 1],
            ),  # no snapshots
        ]:
            assert len(paths) == len(
                paths_exist
            ), "Path must have a corresponding path existence"
            mock_subvol = self._create_test_repo_nspawn_plugins_subvol(
                paths, paths_exist
            )

            self._check_repo_nspawn_plugins(
                attach_antlir_dir,
                shadow_proxied_binaries,
                mock_subvol,
                user,
            )

            assert len(paths) == len(
                paths_check_count
            ), "Path must have a corresponding call count"
            for check_count, path in zip(paths_check_count, paths):
                self.assertEqual(
                    check_count, mock_subvol.path(path).exists.call_count
                )

        # Now, let's check automatic shadowing

        mock_subvol = mock.Mock()
        mock_path = mock.Mock()
        mock_path.exists = mock.Mock(side_effect=[True])
        mock_path.listdir = mock.Mock(side_effect=[[Path("fake_dnf")]])
        mock_subvol.path = mock.Mock(side_effect=[mock_path])

        self.assertEqual(
            (
                (
                    "fake_shadow_paths",
                    [
                        shadow_path_t(dst=b"src", src=b"dest"),
                        shadow_path_t(
                            dst=b"fake_dnf",
                            src=RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
                            / "fake_dnf/fake_dnf/bin/fake_dnf",
                        ),
                    ],
                    {b"fake_dnf"}
                    if attach_antlir_dir == AttachAntlirDirMode.DEFAULT_ON
                    else set(),
                ),
                (
                    "fake_repo_server",
                    {
                        "explicit_snap",
                        RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / "fake_dnf",
                    },
                ),
            ),
            repo_plugins.repo_nspawn_plugins(
                opts=new_nspawn_opts(
                    cmd=[], layer=mock_subvol, user=pwd.getpwnam("root")
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=True,
                    # These are here to show that our shadowing defaults do
                    # not break explicitly requested inputs.
                    shadow_paths=[shadow_path_t(dst=b"src", src=b"dest")],
                    serve_rpm_snapshots=["explicit_snap"],
                ),
            ),
        )

        mock_subvol.path.assert_called_once_with(
            RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
        )
        mock_path.exists.assert_called_once_with()
        mock_path.listdir.assert_called_once_with()

    def test_nspawn_antlir_exists(self):
        subvol = layer_resource_subvol(__package__, "build-appliance")
        self.assertEqual(
            (),
            repo_plugins.repo_nspawn_plugins(
                opts=new_nspawn_opts(
                    cmd=[], layer=subvol, user=pwd.getpwnam("root")
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=False,
                    attach_antlir_dir=AttachAntlirDirMode.DEFAULT_ON,
                ),
            ),
        )

    @with_mock_plugins
    def test_shadow_paths_allow_unmatched(self):
        subvol = layer_resource_subvol(__package__, "build-appliance")
        rpm_installers = {Path(rpm) for rpm in get_rpm_installers_supported()}

        plugins = (
            repo_plugins.repo_nspawn_plugins(
                opts=new_nspawn_opts(
                    cmd=[], layer=subvol, user=pwd.getpwnam("root")
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=True,
                    attach_antlir_dir=AttachAntlirDirMode.DEFAULT_ON,
                ),
            ),
        )

        self.assertEqual(
            (
                (
                    (
                        "fake_shadow_paths",
                        [
                            shadow_path_t(
                                dst=rpm,
                                src=RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
                                / f"{rpm}/{rpm}/bin/{rpm}",
                            )
                            for rpm in sorted(rpm_installers)
                        ],
                        rpm_installers,
                    ),
                    (
                        "fake_repo_server",
                        {
                            RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / rpm
                            for rpm in rpm_installers
                        },
                    ),
                ),
            ),
            plugins,
        )
