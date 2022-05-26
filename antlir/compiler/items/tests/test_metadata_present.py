# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from unittest import TestCase

from antlir.config import repo_config

_TEST_TARGET_PATH = (
    "fbcode//antlir/compiler/items/tests:test-metadata-present__test_layer"
)


class TestMetadataPresent(TestCase):
    def test_metadata_prsent(self):
        with open("/.meta/build/target") as f:
            self.assertEqual(f.readline().strip(), _TEST_TARGET_PATH)

        with open("/.meta/build/revision") as f:
            self.assertEqual(f.readline().strip(), repo_config().vcs_revision)
