# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
import unittest.mock

from antlir.artifacts_dir import find_repo_root
from antlir.config import load_repo_config, repo_config_t
from antlir.fs_utils import Path, temp_dir


class RepoConfigTestCase(unittest.TestCase):
    def _test_repo_config_json(self, **kwargs) -> str:
        # These are just hand constructed values that have no
        # real meaning outside of the context of this test case.
        defaults = {
            "artifacts_require_repo": True,
            "build_appliance_default": "//build/appliance:default",
            "host_mounts_for_repo_artifacts": [],
            "rpm_installer_default": "yum",
            "rpm_installers_supported": [
                "yum",
                "dnf",
            ],
            "version_set_to_path": {
                "all_jobs": "//all/jobs/version/set:path",
            },
            "version_set_default": "all_jobs",
        }
        defaults.update(kwargs)
        return repo_config_t(**defaults).json(exclude={"repo_root": ...})

    def test_repo_config(self):
        config = load_repo_config()

        self.assertIsInstance(config, repo_config_t)
        # The build_appliance_default config attribute
        # really needs to exist and be set to something
        # other than empty string.  While we don't explicitly
        # *require* this field to be provided many things in
        # this tool will blow up if it's not set, so it is, for
        # all intents and purposes, required.  So, we use that
        # fact as a simple unit test to ensure the repo_config_t
        # can load properly.
        self.assertIsNotNone(config.build_appliance_default)
        self.assertNotEqual(config.build_appliance_default, "")

        self.assertEqual(config.repo_root, find_repo_root())

    @unittest.mock.patch("antlir.config._read_text")
    def test_repo_config_artifacts_require_repo_false(self, _read_text):
        # Test case for loading the config and finding the repo root
        # where the artifacts don't require the repo.  This is a possible
        # case if the binaries are built standalone (mode/opt internally).
        # It is reasonable to assume that we may not have a code repository
        # on disk in this case.

        # Generate data to be loaded by the load_repo_config method
        _read_text.return_value = self._test_repo_config_json(
            artifacts_require_repo=False,
        )

        # To force the lack of a repo, we need to set the `path_in_repo` to /
        # so we are ensured to never find a repo.
        config = load_repo_config(path_in_repo=Path("/"))
        self.assertIsInstance(config, repo_config_t)

        # We shouldn't have a repository root
        self.assertIsNone(config.repo_root)

    @unittest.mock.patch("antlir.config._read_text")
    def test_repo_config_fail_artifacts_require_repo_true(self, _read_text):
        # Test the case where the artifacts require the repo, but we can't
        # find it.

        # Force the value of `artifacts_require_repo` to True so we can force
        # the error we are testing for
        _read_text.return_value = self._test_repo_config_json(
            artifacts_require_repo=True,
        )

        with self.assertRaises(RuntimeError):
            load_repo_config(path_in_repo=Path("/"))

    @unittest.mock.patch("antlir.config.find_artifacts_dir")
    @unittest.mock.patch("antlir.config._read_text")
    def test_repo_config_host_mounts(self, _read_text, artifacts_dir_mock):
        # Force the value of `artifacts_require_repo` to True so we can force
        # looking for the artifacts_dir
        _read_text.return_value = self._test_repo_config_json(
            artifacts_require_repo=True,
        )

        with temp_dir() as td:
            mock_backing_dir = td / "backing-dir"
            mock_artifact_dir = Path(td / "buck-image-out")
            os.symlink(mock_backing_dir, mock_artifact_dir)

            artifacts_dir_mock.return_value = mock_artifact_dir

            config = load_repo_config()

            # Note: this has to be a string because the `config_t` shape doesn't
            # understand the Path type yet
            self.assertIn(
                str(mock_backing_dir), config.host_mounts_for_repo_artifacts
            )
