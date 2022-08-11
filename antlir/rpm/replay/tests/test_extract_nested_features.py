#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest

from antlir.config import antlir_dep

from antlir.rpm.replay.extract_nested_features import log as enf_log
from antlir.rpm.replay.tests.test_utils import (
    build_env_map,
    extract_features_from_env_map,
)


def _extract_features(infix: str):
    return extract_features_from_env_map(build_env_map(os.environ, infix))


class ExtractNestedFeaturesTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        unittest.util._MAX_LENGTH = 12345
        cls.maxDiff = None

    def _check_base_plus_one(self, ef) -> None:
        # There are a bunch of empty `image_source` implementation fields
        # here that we don't want to assert since that'd be fragile.
        lfp = ef.packaged_root.layer_from_package
        self.assertEqual("sendstream", lfp["format"])
        self.assertIsNone(lfp["source"]["path"])

        # This would only exit in the test's base, but not on a host running us
        self.assertTrue(
            ef.packaged_root.layer.path("/from/test_base/").exists()
        )

        self.assertEqual({"rpm-test-milk"}, ef.install_rpm_names)

        self.assertTrue(
            os.path.exists(ef.packaged_root.layer.path("/from/test_base/"))
        )

    def test_custom(self) -> None:
        ef = _extract_features("custom")
        # Omits "/from/test_base/" since it's in the base image.
        self.assertEqual({"/new/dir/"}, ef.make_dir_paths)
        self._check_base_plus_one(ef)
        self.assertEqual({"remove_paths"}, ef.features_needing_custom_image)
        self.assertIsNone(ef.features_to_replay)  # Deliberately not set!

    def test_custom_remove_rpm(self) -> None:
        with self.assertLogs(enf_log, level="ERROR") as ctx:
            ef = _extract_features("custom-remove-rpm")
        self.assertIn(' besides "install" need a custom ', "".join(ctx.output))
        self.assertEqual({"/new/dir/"}, ef.make_dir_paths)
        self._check_base_plus_one(ef)
        self.assertEqual({"rpms"}, ef.features_needing_custom_image)
        self.assertIsNone(ef.features_to_replay)

    def test_custom_local_rpm(self) -> None:
        with self.assertLogs(enf_log, level="ERROR") as ctx:
            ef = _extract_features("custom-local-rpm")
        self.assertIn("Installing an in-repo RPM ", "".join(ctx.output))
        self.assertEqual({"/new/dir/"}, ef.make_dir_paths)
        self._check_base_plus_one(ef)
        self.assertEqual({"rpms"}, ef.features_needing_custom_image)
        self.assertIsNone(ef.features_to_replay)

    def test_non_custom(self) -> None:
        ef = _extract_features("non-custom")
        self.assertEqual({"/new/dir/", "/another/dir/"}, ef.make_dir_paths)
        self._check_base_plus_one(ef)
        self.assertEqual(set(), ef.features_needing_custom_image)

        def feature_target(layer_name):
            return antlir_dep(f"rpm/replay/tests:{layer_name}__layer-feature")

        self.assertEqual(
            {
                ("layer_from_package", feature_target("base")),
                ("parent_layer", feature_target("base-plus-one")),
                ("mounts", feature_target("base-plus-one")),
                ("rpms", feature_target("base-plus-one")),
                ("ensure_subdirs_exist", feature_target("base-plus-one")),
                ("parent_layer", feature_target("non-custom")),
                ("ensure_subdirs_exist", feature_target("non-custom")),
                ("meta_key_value_store", feature_target("non-custom")),
            },
            {(key, target) for key, target, _cfg in ef.features_to_replay},
        )
