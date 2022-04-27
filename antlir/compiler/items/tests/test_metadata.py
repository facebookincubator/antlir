# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from unittest.mock import patch

from antlir.config import repo_config
from antlir.subvol_utils import TempSubvolumes

from ..common import PhaseOrder
from ..metadata import LayerInfoItem
from .common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
)


class TestMetadataLayerInfoItem(BaseItemTestCase):
    def test_layer_info_item_phase_order(self):
        layer_info_item = LayerInfoItem(
            from_target="target",
        )

        self.assertEqual(layer_info_item.phase_order(), PhaseOrder.LAYER_INFO)

    def test_layer_info_builder(self):
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("layer_info_test")
            # Create a wide open /.meta/build to avoid using sudo
            subvol.run_as_root(
                [
                    "mkdir",
                    "--mode=0777",
                    "--parent",
                    subvol.path("/.meta/build"),
                ]
            )

            with patch("getpass.getuser", return_value="root") as p:
                LayerInfoItem.get_phase_builder(
                    [
                        LayerInfoItem(from_target="target"),
                    ],
                    layer_opts=DUMMY_LAYER_OPTS,
                )(subvol)
                p.assert_called_once()

            self.assertEqual(
                subvol.read_path_text_as_root("/.meta/build/target").strip(),
                "fake target",
            )

            self.assertEqual(
                subvol.read_path_text_as_root("/.meta/build/revision").strip(),
                repo_config().vcs_revision,
            )
