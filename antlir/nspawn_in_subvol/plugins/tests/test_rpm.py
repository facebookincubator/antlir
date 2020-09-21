#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd
import unittest
from unittest import mock

from antlir.fs_utils import RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR, Path
from antlir.nspawn_in_subvol.args import NspawnPluginArgs, new_nspawn_opts

from .. import rpm as rpm_plugins


class RpmPluginsTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    # This fully mocked because we have lots of integration tests:
    #   - the per-plugin tests
    #   - `test-rpm-installer-shadow-paths`
    @mock.patch.object(
        rpm_plugins,
        "YumDnfVersionlock",
        mock.Mock(side_effect=lambda x: ("fake_version_lock", x)),
    )
    @mock.patch.object(
        rpm_plugins,
        "RepoServers",
        mock.Mock(side_effect=lambda x: ("fake_repo_server", x)),
    )
    @mock.patch.object(
        rpm_plugins,
        "ShadowPaths",
        mock.Mock(side_effect=lambda x: ("fake_shadow_paths", x)),
    )
    def test_rpm_nspawn_plugins(self):
        mock_subvol = mock.Mock(spec=["canonicalize_path"])
        mock_subvol.canonicalize_path = mock.Mock(
            side_effect=lambda x: x / Path("_")
        )

        # None of these will trigger automatic shadowing
        for shadow_proxied_binaries, user, snapshots_exist in [
            (False, pwd.getpwnam("root"), True),  # disabled
            (True, pwd.getpwnam("nobody"), True),  # not root
            (True, pwd.getpwnam("root"), False),  # no snapshots
        ]:
            mock_path = mock.Mock()
            mock_path.exists = mock.Mock(side_effect=[snapshots_exist])
            mock_subvol.path = mock.Mock(side_effect=[mock_path])

            self.assertEqual(
                (
                    ("fake_shadow_paths", [("src", "dest")]),
                    ("fake_version_lock", {b"a/_": "vla", b"c/_": "vlc"}),
                    ("fake_repo_server", {b"a/_", b"b/_", b"c/_"}),
                ),
                rpm_plugins.rpm_nspawn_plugins(
                    opts=new_nspawn_opts(cmd=[], layer=mock_subvol, user=user),
                    plugin_args=NspawnPluginArgs(
                        shadow_proxied_binaries=shadow_proxied_binaries,
                        shadow_paths=[("src", "dest")],
                        serve_rpm_snapshots=("a", "b", "c"),
                        snapshots_and_versionlocks=[("a", "vla"), ("c", "vlc")],
                    ),
                ),
            )

            mock_subvol.path.assert_called_once_with(
                RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
            )
            if snapshots_exist:
                mock_path.exists.assert_not_called()
            else:
                mock_path.exists.assert_called_once_with()

        # Now, let's check automatic shadowing

        mock_path = mock.Mock()
        mock_path.exists = mock.Mock(side_effect=[True])
        mock_path.listdir = mock.Mock(side_effect=[[Path("fake_dnf")]])
        mock_subvol.path = mock.Mock(side_effect=[mock_path])

        self.assertEqual(
            (
                (
                    "fake_shadow_paths",
                    [
                        ("src", "dest"),
                        (
                            b"fake_dnf",
                            RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
                            / "fake_dnf/fake_dnf/bin/fake_dnf",
                        ),
                    ],
                ),
                (
                    "fake_repo_server",
                    {
                        b"explicit_snap/_",
                        RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / "fake_dnf/_",
                    },
                ),
            ),
            rpm_plugins.rpm_nspawn_plugins(
                opts=new_nspawn_opts(
                    cmd=[], layer=mock_subvol, user=pwd.getpwnam("root")
                ),
                plugin_args=NspawnPluginArgs(
                    shadow_proxied_binaries=True,
                    # These are here to show that our shadowing defaults do
                    # not break explicitly requested inputs.
                    shadow_paths=[("src", "dest")],
                    serve_rpm_snapshots=["explicit_snap"],
                ),
            ),
        )

        mock_subvol.path.assert_called_once_with(
            RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
        )
        mock_path.exists.assert_called_once_with()
        mock_path.listdir.assert_called_once_with()
