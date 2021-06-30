# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest

from antlir.compiler.items_for_features import ItemFactory
from antlir.config import load_repo_config
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.rpm.replay.extract_nested_features import (
    extract_nested_features,
)
from antlir.rpm.replay.rpm_replay import (
    replay_rpms_and_compiler_items,
    filter_features_to_replay,
)
from antlir.rpm.replay.subvol_diff import subvol_diff
from antlir.rpm.replay.subvol_rpm_compare import (
    subvol_rpm_compare_and_download,
    SubvolsToCompare,
)
from antlir.rpm.replay.tests.test_utils import build_env_map
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.tests.layer_resource import layer_resource_subvol


class RpmReplayTestCase(unittest.TestCase):
    def test_replay_rpms_and_compiler_items(self):
        root = layer_resource_subvol(__package__, "root_subvol")
        leaf = layer_resource_subvol(__package__, "leaf_subvol")
        ba = layer_resource_subvol(__package__, "ba_subvol")

        env_map = build_env_map(os.environ, "leaf")
        extracted_features = extract_nested_features(
            layer_features_out=env_map["layer_feature_json"],
            layer_out=env_map["layer_output"],
            target_to_path=env_map["target_map"],
        )

        def gen_replay_items(exit_stack, layer_opts):
            item_factory = ItemFactory(exit_stack, layer_opts)
            for feature in filter_features_to_replay(
                extracted_features.features_to_replay
            ):
                yield from item_factory.gen_items_for_feature(*feature)

        subvols = SubvolsToCompare(
            ba=ba,
            root=root,
            leaf=leaf,
            rpm_installer=YumDnf(
                load_repo_config().flavor_to_config["antlir_test"].rpm_installer
            ),
            rpm_repo_snapshot=snapshot_install_dir(
                "//antlir/rpm:subvol-rpm-compare-repo-snapshot-for-tests"
            ),
        )

        with subvol_rpm_compare_and_download(subvols) as (
            rpm_diff,
            rpm_download_subvol,
        ):
            with replay_rpms_and_compiler_items(
                rpm_diff=rpm_diff,
                rpm_download_subvol=rpm_download_subvol,
                subvols=subvols,
                flavor="antlir_test",
                artifacts_may_require_repo=True,
                target_to_path=env_map["target_map"],
                gen_replay_items=gen_replay_items,
            ) as install_subvol:
                diff = list(subvol_diff(subvols.leaf, install_subvol))
                self.assertEqual(diff, [])
