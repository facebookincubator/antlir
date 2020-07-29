#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import sys

from fs_image.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_subvols,
)
from fs_image.btrfs_diff.tests.render_subvols import render_sendstream
from fs_image.fs_utils import Path
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder
from ..make_dirs import MakeDirsItem
from ..make_subvol import (
    FilesystemRootItem,
    ParentLayerItem,
    ReceiveSendstreamItem,
)
from .common import BaseItemTestCase, get_dummy_layer_opts_ba, render_subvol


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


class MakeSubvolItemsTestCase(BaseItemTestCase):
    def test_filesystem_root(self):
        item = FilesystemRootItem(from_target="t")
        self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.caller_will_create("fs-root")
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(subvol)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        # TODO(jtru): Remove when meta migration has propagated
                        "meta": ["(Dir)", {}],
                        ".meta": [
                            "(Dir)",
                            {
                                "private": [
                                    "(Dir)",
                                    {
                                        "opts": [
                                            "(Dir)",
                                            {
                                                "artifacts_may_require_repo": [
                                                    "(File d2)"
                                                ]
                                            },
                                        ]
                                    },
                                ]
                            },
                        ],
                    },
                ],
                render_subvol(subvol),
            )

    def test_parent_layer(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            parent = temp_subvolumes.create("parent")
            item = ParentLayerItem(from_target="t", subvol=parent)
            self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())

            MakeDirsItem(
                from_target="t", into_dir="/", path_to_make="a/b"
            ).build(parent, DUMMY_LAYER_OPTS_BA)
            parent_content = ["(Dir)", {"a": ["(Dir)", {"b": ["(Dir)", {}]}]}]
            self.assertEqual(parent_content, render_subvol(parent))

            # Take a snapshot and add one more directory.
            child = temp_subvolumes.caller_will_create("child")
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(child)
            MakeDirsItem(from_target="t", into_dir="a", path_to_make="c").build(
                child, DUMMY_LAYER_OPTS_BA
            )

            # The parent is unchanged.
            self.assertEqual(parent_content, render_subvol(parent))
            child_content = copy.deepcopy(parent_content)
            child_content[1]["a"][1]["c"] = ["(Dir)", {}]
            # TODO(jtru): Remove when meta migration has propagated
            child_content[1]["meta"] = ["(Dir)", {}]
            # Since the parent lacked a /.meta, the child added it.
            child_content[1][".meta"] = [
                "(Dir)",
                {
                    "private": [
                        "(Dir)",
                        {
                            "opts": [
                                "(Dir)",
                                {"artifacts_may_require_repo": ["(File d2)"]},
                            ]
                        },
                    ]
                },
            ]
            self.assertEqual(child_content, render_subvol(child))

    def test_receive_sendstream(self):
        item = ReceiveSendstreamItem(
            from_target="t",
            source=Path(__file__).dirname() / "create_ops.sendstream",
        )
        self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            new_subvol_name = "differs_from_create_ops"
            subvol = temp_subvolumes.caller_will_create(new_subvol_name)
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(subvol)
            self.assertEqual(
                render_demo_subvols(create_ops=new_subvol_name),
                render_sendstream(subvol.mark_readonly_and_get_sendstream()),
            )
