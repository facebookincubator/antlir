#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from unittest import mock

from .. import rpm_plugins


class RpmPluginsTestCase(unittest.TestCase):

    # This fully mocked because `test-run` does the integration testing.
    @mock.patch.object(
        rpm_plugins, 'nspawn_plugin_to_inject_yum_dnf_versionlock',
        mock.Mock(side_effect=lambda x: ('test_vl', x)),
    )
    @mock.patch.object(
        rpm_plugins, 'nspawn_plugin_to_inject_repo_servers',
        mock.Mock(side_effect=lambda x: ('test_rs', x)),
    )
    def test_nspawn_rpm_plugins(self):
        self.assertEqual(
            (
                ('test_vl', {'a': 'vla', 'c': 'vlc'}),
                ('test_rs', {'a', 'b', 'c'}),
            ),
            rpm_plugins.nspawn_rpm_plugins(
                serve_rpm_snapshots=('a', 'b', 'c'),
                snapshots_and_versionlocks=[('a', 'vla'), ('c', 'vlc')],
            )
        )
