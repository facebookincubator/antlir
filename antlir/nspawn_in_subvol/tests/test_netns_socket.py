# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import socket
import subprocess
from unittest.mock import patch

from antlir.nspawn_in_subvol.netns_socket import (
    _make_sockets_and_send_via,
    create_sockets_inside_netns,
)

from antlir.tests.common import AntlirTestCase


class NetnsSocketTestCase(AntlirTestCase):
    """
    This class provides partial coverage.
    Full intergartion test is done in test-repo-servers.
    """

    @patch(
        "antlir.nspawn_in_subvol.netns_socket.recv_fds_from_unix_sock",
    )
    @patch("antlir.nspawn_in_subvol.netns_socket._mockable_popen")
    def test_create_sockets_inside_netns(self, s_patch, r_patch) -> None:
        expected_cmd = [
            "sudo",
            "env",
            "PATH=/usr/local/fbcode/bin:/bin",
            "nsenter",
            "--net",
            "--target",
            "1",
        ]
        expected_cmd.extend(
            _make_sockets_and_send_via(num_socks=1, unix_sock_fd=1)
        )
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)

        s_patch.side_effect = lambda *args, **kwargs: subprocess.Popen("true")

        r_patch.return_value = [s.fileno()]
        self.assertIsInstance(
            create_sockets_inside_netns(1, 1)[0], socket.socket
        )
        s_patch.assert_called_once()
        self.assertEqual(s_patch.call_args[0][0][:-1], expected_cmd[:-1])
