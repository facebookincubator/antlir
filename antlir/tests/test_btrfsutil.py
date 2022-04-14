#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import os.path
import sys
from unittest.mock import patch

import btrfsutil as _raw_btrfsutil  # pyre-ignore[21]
from antlir import btrfsutil
from antlir.fs_utils import Path

from ..artifacts_dir import ensure_per_repo_artifacts_dir_exists
from ..subvol_utils import with_temp_subvols
from ..volume_for_repo import get_volume_for_current_repo
from .common import AntlirTestCase


class BtrfsUtilTestCase(AntlirTestCase):
    def setUp(self):
        super().setUp()
        # Make sure we have a volume to work with
        get_volume_for_current_repo(
            ensure_per_repo_artifacts_dir_exists(Path(sys.argv[0]))
        )

    @patch("antlir.btrfsutil._raw_btrfsutil")
    @patch("antlir.btrfsutil._sudo_retry")
    @with_temp_subvols
    def test_no_sudo_necessary(self, temp_subvols, sudo_retry, raw_btrfsutil):
        """Does not use sudo when underlying call passes"""
        subvol = temp_subvols.create("no_sudo_necessary")
        raw_btrfsutil.subvolume_info.__name__ = "subvolume_info"
        btrfsutil.subvolume_info(subvol.path())
        raw_btrfsutil.subvolume_info.assert_called_once()
        sudo_retry.assert_not_called()

    @patch("antlir.btrfsutil._raw_btrfsutil")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_non_perm_error(self, sudo_retry, raw_btrfsutil):
        """Non-permission errors are not retried"""
        raw_btrfsutil.subvolume_info.__name__ = "subvolume_info"
        raw_btrfsutil.subvolume_info.side_effect = (
            _raw_btrfsutil.BtrfsUtilError(errno.ENOENT, None)
        )
        with self.assertRaises(_raw_btrfsutil.BtrfsUtilError) as cm:
            btrfsutil.subvolume_info("fake-path")
        self.assertEqual(cm.exception.errno, errno.ENOENT)
        raw_btrfsutil.subvolume_info.assert_called_once()
        sudo_retry.assert_not_called()

    @patch("antlir.btrfsutil._raw_btrfsutil")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_sudo_fallback(self, sudo_retry, raw_btrfsutil):
        """Falls back to calling sudo on permissions errors"""
        raw_btrfsutil.subvolume_info.__name__ = "subvolume_info"
        raw_btrfsutil.subvolume_info.side_effect = (
            _raw_btrfsutil.BtrfsUtilError(errno.EPERM, None)
        )
        btrfsutil.subvolume_info("fake-path")
        raw_btrfsutil.subvolume_info.assert_called_once()
        sudo_retry.assert_called_once()

    @patch("antlir.btrfsutil._raw_btrfsutil")
    @patch("antlir.btrfsutil._sudo_retry")
    def test_sudo_fallback_fails(self, sudo_retry, raw_btrfsutil):
        """Exception from _sudo_retry gets raised"""
        raw_btrfsutil.subvolume_info.__name__ = "subvolume_info"
        raw_btrfsutil.subvolume_info.side_effect = (
            _raw_btrfsutil.BtrfsUtilError(errno.EPERM, None)
        )
        sudo_retry.side_effect = RuntimeError("test")
        with self.assertRaises(RuntimeError, msg="test"):
            btrfsutil.subvolume_info("fake-path")
        raw_btrfsutil.subvolume_info.assert_called_once()
        sudo_retry.assert_called_once()

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

    @patch("antlir.btrfsutil._raw_btrfsutil")
    @patch("antlir.btrfsutil._sudo_retry", wraps=btrfsutil._sudo_retry)
    @with_temp_subvols
    def test_integration_sudo_fallback(
        self, temp_subvols, sudo_retry, raw_btrfsutil
    ):
        """Sudo fallback works"""
        subvol = temp_subvols.create("sudo_fallback")
        raw_btrfsutil.subvolume_info.__name__ = "subvolume_info"
        raw_btrfsutil.subvolume_info.side_effect = (
            _raw_btrfsutil.BtrfsUtilError(errno.EPERM, None)
        )
        btrfsutil.subvolume_info(subvol.path())
        raw_btrfsutil.subvolume_info.assert_called_once()
        sudo_retry.assert_called_once()
