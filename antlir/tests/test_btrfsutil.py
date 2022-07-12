#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import os.path
import sys
from unittest.mock import MagicMock, patch

import btrfsutil as _raw_btrfsutil  # pyre-ignore[21]
from antlir import btrfsutil

from antlir.artifacts_dir import ensure_per_repo_artifacts_dir_exists
from antlir.fs_utils import Path
from antlir.subvol_utils import with_temp_subvols
from antlir.tests.common import AntlirTestCase
from antlir.unshare import Namespace, Unshare
from antlir.volume_for_repo import get_volume_for_current_repo


class BtrfsUtilTestCase(AntlirTestCase):
    def setUp(self):
        super().setUp()
        # Make sure we have a volume to work with
        get_volume_for_current_repo(
            ensure_per_repo_artifacts_dir_exists(Path(sys.argv[0]))
        )

    @with_temp_subvols
    def test_no_sudo_necessary(self, temp_subvols):
        """Does not use sudo when underlying call passes"""
        subvol = temp_subvols.create("no_sudo_necessary")
        with patch(
            "antlir.btrfsutil._raw_btrfsutil.subvolume_info"
        ) as mock_subvolume_info, patch(
            "antlir.btrfsutil._sudo_retry"
        ) as mock_sudo_retry:
            btrfsutil.subvolume_info(subvol.path())
        mock_subvolume_info.assert_called_once()
        mock_sudo_retry.assert_not_called()

    @patch("antlir.btrfsutil._raw_btrfsutil.subvolume_info")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_non_perm_error(self, mock_sudo_retry, mock_subvolume_info):
        """Non-permission errors are not retried"""
        mock_subvolume_info.side_effect = _raw_btrfsutil.BtrfsUtilError(
            errno.ENOENT, None
        )
        with self.assertRaises(_raw_btrfsutil.BtrfsUtilError) as cm:
            btrfsutil.subvolume_info("fake-path")
        self.assertEqual(cm.exception.errno, errno.ENOENT)
        mock_subvolume_info.assert_called_once()
        mock_sudo_retry.assert_not_called()

    @patch("antlir.btrfsutil._raw_btrfsutil.subvolume_info")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_sudo_fallback(self, mock_sudo_retry, mock_subvolume_info):
        """Falls back to calling sudo on permissions errors"""
        mock_subvolume_info.__name__ = "subvolume_info"
        mock_subvolume_info.side_effect = _raw_btrfsutil.BtrfsUtilError(
            errno.EPERM, None
        )
        btrfsutil.subvolume_info("fake-path")
        mock_subvolume_info.assert_called_once()
        mock_sudo_retry.assert_called_once()

    @patch("antlir.btrfsutil._raw_btrfsutil.subvolume_info")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_sudo_fallback_fails(self, mock_sudo_retry, mock_subvolume_info):
        """Exception from _sudo_retry gets raised"""
        mock_subvolume_info.__name__ = "subvolume_info"
        mock_subvolume_info.side_effect = _raw_btrfsutil.BtrfsUtilError(
            errno.EPERM, None
        )
        mock_sudo_retry.side_effect = RuntimeError("test")
        with self.assertRaises(RuntimeError, msg="test"):
            btrfsutil.subvolume_info("fake-path")
        mock_subvolume_info.assert_called_once()
        mock_sudo_retry.assert_called_once()

    def test_sudo_fallback_subprocess_exception(self):
        """Exception in _sudo_retry subprocess is raised"""
        # this name must be unittest_fail
        def unittest_fail():
            pass

        with self.assertRaises(
            RuntimeError, msg="failing for unittest coverage"
        ):
            btrfsutil._sudo_retry(unittest_fail, None, None)

    @patch("antlir.btrfsutil._sudo_retry", wraps=btrfsutil._sudo_retry)
    @with_temp_subvols
    def test_integration_unpriv(self, temp_subvols, sudo_retry):
        """Regular unprivileged calls work"""
        to_create = temp_subvols.temp_dir / "create-subvol-unpriv"
        self.assertFalse(os.path.exists(to_create), to_create)
        # this runs through sudo anyway
        btrfsutil.create_subvolume(to_create)
        sudo_retry.assert_called_once()
        sudo_retry.reset_mock()
        # but calling btrfsutil.is_subvolume does not
        self.assertTrue(btrfsutil.is_subvolume(to_create))
        sudo_retry.assert_not_called()

    @patch("antlir.btrfsutil._raw_btrfsutil.subvolume_info")
    @with_temp_subvols
    def test_integration_sudo_fallback(self, temp_subvols, mock_subvolume_info):
        """Sudo fallback works"""
        subvol = temp_subvols.create("sudo_fallback")
        mock_subvolume_info.__name__ = "subvolume_info"
        mock_subvolume_info.side_effect = _raw_btrfsutil.BtrfsUtilError(
            errno.EPERM, None
        )
        with patch("antlir.btrfsutil._sudo_retry") as mock_sudo_retry:
            btrfsutil.subvolume_info(subvol.path())
        mock_subvolume_info.assert_called_once()
        mock_sudo_retry.assert_called_once()

    @patch("antlir.btrfsutil._raw_btrfsutil.subvolume_info")
    @with_temp_subvols
    def test_in_namespace(self, temp_subvols, mock_subvolume_info):
        """in_namespace calls nsenter"""
        subvol = temp_subvols.create("sudo_fallback")
        mock_subvolume_info.__name__ = "subvolume_info"
        with Unshare([Namespace.PID]) as ns:
            with patch(
                "antlir.btrfsutil._sudo_retry", wraps=btrfsutil._sudo_retry
            ) as mock_sudo_retry:
                btrfsutil.subvolume_info(subvol.path(), in_namespace=ns)
        mock_subvolume_info.assert_not_called()
        mock_sudo_retry.assert_called_once()
