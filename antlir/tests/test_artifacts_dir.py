# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import tempfile
import unittest

from antlir.artifacts_dir import find_repo_root
from antlir.fs_utils import Path


class ArtifactsDirTests(unittest.TestCase):
    def test_git_repo_root(self):
        with tempfile.TemporaryDirectory() as td:
            td = Path(td)
            # Make the td the repo root
            os.makedirs(td / b".git")

            # A subdir to start from, a very good place to start.
            repo_subdir = td / "i/am/a/subdir/of/the/repo"
            os.makedirs(repo_subdir)

            # Git has submodules, make one of those like git does
            repo_submodule_subdir = td / "i/am/a/submodule/subdir"
            os.makedirs(repo_submodule_subdir)
            Path(repo_submodule_subdir.dirname() / ".git").touch()

            # Check all the possible variations
            self.assertEqual(find_repo_root(path_in_repo=td), td)
            self.assertEqual(find_repo_root(path_in_repo=repo_subdir), td)
            self.assertEqual(
                find_repo_root(path_in_repo=repo_submodule_subdir), td
            )

    def test_hg_repo_root(self):
        with tempfile.TemporaryDirectory() as td:
            td = Path(td)
            # Make the td the repo root
            os.makedirs(td / b".hg")

            repo_subdir = td / "i/am/a/subdir/of/the/repo"
            os.makedirs(repo_subdir)

            # Check all the possible variations
            self.assertEqual(find_repo_root(path_in_repo=td), td)
            self.assertEqual(find_repo_root(path_in_repo=repo_subdir), td)
