# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest

from antlir.compiler.items_for_features import ItemFactory
from antlir.config import antlir_dep, repo_config
from antlir.rpm.find_snapshot import snapshot_install_dir

from antlir.rpm.replay.rpm_replay import (
    filter_features_to_replay,
    LayerOpts,
    replay_rpms_and_compiler_items,
)
from antlir.rpm.replay.subvol_diff import subvol_diff
from antlir.rpm.replay.subvol_rpm_compare import (
    subvol_rpm_compare_and_download,
    SubvolsToCompare,
)
from antlir.rpm.replay.tests.test_utils import (
    build_env_map,
    extract_features_from_env_map,
)
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.tests.layer_resource import layer_resource_subvol


class RpmReplayTestCase(unittest.TestCase):
    def test_replay_rpms_and_compiler_items(self):
        root = layer_resource_subvol(__package__, "root_subvol")
        leaf = layer_resource_subvol(__package__, "leaf_subvol")
        ba = layer_resource_subvol(__package__, "ba_subvol")

        env_map = build_env_map(os.environ, "leaf")
        extracted_features = extract_features_from_env_map(env_map)

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
                repo_config().flavor_to_config["antlir_test"].rpm_installer
            ),
            rpm_repo_snapshot=snapshot_install_dir(
                antlir_dep("rpm:rpm-replay-repo-snapshot-for-tests")
            ),
        )

        with subvol_rpm_compare_and_download(subvols) as (
            rpm_diff,
            rpm_download_subvol,
        ):
            with replay_rpms_and_compiler_items(
                rpm_diff=rpm_diff,
                rpm_download_subvol=rpm_download_subvol,
                root=root,
                layer_opts=LayerOpts(
                    artifacts_may_require_repo=repo_config().artifacts_require_repo,
                    build_appliance=ba,
                    layer_target="unimportant",
                    rpm_installer=subvols.rpm_installer,
                    flavor="antlir_test",
                    rpm_repo_snapshot=subvols.rpm_repo_snapshot,
                    target_to_path=env_map["target_map"],
                    subvolumes_dir=None,
                    version_set_override=None,
                    debug=True,
                ),
                gen_replay_items=gen_replay_items,
                compile_items_to_subvol_bin_path=os.environ[
                    "compile_items_to_subvol_bin_path"
                ],
            ) as install_subvol:
                diff = list(subvol_diff(subvols.leaf, install_subvol))
                self.assertEqual(diff, [])
