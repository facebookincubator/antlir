# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.config import load_repo_config, repo_config_t


class RepoConfigTestCase(unittest.TestCase):
    def test_repo_config(self):
        config = load_repo_config()

        self.assertIsInstance(config, repo_config_t)
        # The build_appliance_default config attribute
        # really needs to exist and be set to something
        # other than empty string.  While we don't explicitly
        # *require* this field to be provided many things in
        # this tool will blow up if it's not set, so it is, for
        # all intents and purposes, required.  So, we use that
        # fact as a simple unit test to ensure the RepoConfig
        # can load properly.
        self.assertIsNotNone(config.build_appliance_default)
        self.assertNotEqual(config.build_appliance_default, "")
