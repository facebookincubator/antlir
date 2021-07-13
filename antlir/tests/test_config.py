# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
import unittest.mock

from antlir.artifacts_dir import find_repo_root
from antlir.config import (
    base_repo_config_t,
    repo_config,
    repo_config_t,
    _unmemoized_repo_config,
)
from antlir.fs_utils import Path, temp_dir


class RepoConfigTestCase(unittest.TestCase):
    def _test_repo_config(self, **kwargs):
        # These are just hand constructed values that have no
        # real meaning outside of the context of this test case.
        # NOTE: this is the config _after_ being parsed from .buckconfig, not
        # the raw values
        defaults = {
            "artifacts_require_repo": True,
            "artifact": {
                "test.artifact": "//build:artifact",
            },
            "flavor_available": ["no_vset", "with_vset"],
            "flavor_default": "no_vset",
            "antlir_linux_flavor": "no_vset",
            "flavor_to_config": {
                "no_vset": {
                    "name": "no_vset",
                    "rpm_installer": "yum",
                },
                "with_vset": {
                    "name": "with_vset",
                    "version_set_path": "//some/project/path",
                    "rpm_installer": "dnf",
                },
            },
            "host_mounts_allowed_in_targets": [],
            "host_mounts_for_repo_artifacts": [],
        }
        defaults.update(kwargs)
        return base_repo_config_t(**defaults).dict()

    def test_repo_config(self):
        config = repo_config()
        self.assertIs(config, repo_config())  # memoized!
        self.assertIsInstance(config, repo_config_t)
        self.assertEqual(config.repo_root, find_repo_root())

    @unittest.mock.patch("antlir.config.repo_config_data")
    def test_repo_config_artifacts_require_repo_false(self, mock_data):
        # Test case for loading the config and finding the repo root
        # where the artifacts don't require the repo.  This is a possible
        # case if the binaries are built standalone (mode/opt internally).
        # It is reasonable to assume that we may not have a code repository
        # on disk in this case.

        # Generate data to be loaded by the repo_config() method
        mock_data.dict = unittest.mock.Mock()
        mock_data.dict.return_value = self._test_repo_config(
            artifacts_require_repo=False,
        )

        # To force the lack of a repo, we need to set the `path_in_repo` to /
        # so we are ensured to never find a repo.
        config = _unmemoized_repo_config(path_in_repo=Path("/"))
        self.assertIsInstance(config, repo_config_t)

        # We shouldn't have a repository root
        self.assertIsNone(config.repo_root)

    @unittest.mock.patch("antlir.config.repo_config_data")
    def test_repo_config_fail_artifacts_require_repo_true(self, mock_data):
        # Test the case where the artifacts require the repo, but we can't
        # find it.

        # Force the value of `artifacts_require_repo` to True so we can force
        # the error we are testing for
        mock_data.dict = unittest.mock.Mock()
        mock_data.dict.return_value = self._test_repo_config(
            artifacts_require_repo=True,
        )

        with self.assertRaises(RuntimeError):
            _unmemoized_repo_config(path_in_repo=Path("/"))

    @unittest.mock.patch("antlir.config.find_artifacts_dir")
    @unittest.mock.patch("antlir.config.repo_config_data")
    def test_repo_config_host_mounts(self, mock_data, artifacts_dir_mock):
        # Force the value of `artifacts_require_repo` to True so we can force
        # looking for the artifacts_dir
        mock_data.dict = unittest.mock.Mock()
        mock_data.dict.return_value = self._test_repo_config(
            artifacts_require_repo=True,
        )

        with temp_dir() as td:
            mock_backing_dir = td / "backing-dir"
            mock_artifact_dir = Path(td / "buck-image-out")
            os.symlink(mock_backing_dir, mock_artifact_dir)

            artifacts_dir_mock.return_value = mock_artifact_dir

            # Note: this has to be a string because the `config_t` shape doesn't
            # understand the Path type yet
            self.assertIn(
                mock_backing_dir,
                _unmemoized_repo_config().host_mounts_for_repo_artifacts,
            )
