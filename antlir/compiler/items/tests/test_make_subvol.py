#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import sys

from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_subvols,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.subvol_helpers import get_meta_dir_contents, pop_path

from ..common import PhaseOrder
from ..ensure_dirs_exist import (
    EnsureDirsExistItem,
    ensure_subdirs_exist_factory,
)
from ..make_subvol import (
    _check_parent_flavor,
    FilesystemRootItem,
    LayerFromPackageItem,
    ParentLayerItem,
)
from .common import BaseItemTestCase, get_dummy_layer_opts_ba, render_subvol


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


class MakeSubvolItemsTestCase(BaseItemTestCase):
    def test_filesystem_root(self):
        item = FilesystemRootItem(from_target="t")
        self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            subvol = temp_subvolumes.caller_will_create("fs-root")
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(subvol)
            self.assertEqual(
                [
                    "(Dir)",
                    {".meta": get_meta_dir_contents()},
                ],
                render_subvol(subvol),
            )

    def test_parent_layer(self):
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            parent = temp_subvolumes.create("parent")
            item = ParentLayerItem(from_target="t", subvol=parent)
            self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())

            for ede_item in reversed(
                list(
                    ensure_subdirs_exist_factory(
                        from_target="t", into_dir="/", subdirs_to_create="a/b"
                    )
                )
            ):
                ede_item.build(parent, DUMMY_LAYER_OPTS_BA)

            parent_content = ["(Dir)", {"a": ["(Dir)", {"b": ["(Dir)", {}]}]}]
            self.assertEqual(parent_content, render_subvol(parent))

            # Take a snapshot and add one more directory.
            child = temp_subvolumes.caller_will_create("child")
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(child)
            EnsureDirsExistItem(
                from_target="t", into_dir="a", basename="c"
            ).build(child, DUMMY_LAYER_OPTS_BA)

            # The parent is unchanged.
            self.assertEqual(parent_content, render_subvol(parent))
            child_content = copy.deepcopy(parent_content)
            child_content[1]["a"][1]["c"] = ["(Dir)", {}]
            # Since the parent lacked a /.meta, the child added it.
            child_content[1][".meta"] = get_meta_dir_contents()
            self.assertEqual(child_content, render_subvol(child))

    def test_parent_layer_flavor_error(self):
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            parent = layer_resource_subvol(__package__, "test-build-appliance")
            item = ParentLayerItem(from_target="t", subvol=parent)

            child = temp_subvolumes.caller_will_create("child")
            with self.assertRaisesRegex(
                AssertionError, "does not match provided flavor"
            ):
                item.get_phase_builder(
                    [item],
                    DUMMY_LAYER_OPTS_BA._replace(flavor="different flavor"),
                )(child)

    def _check_receive_package(self, item, lossy_packaging=None):
        self.assertEqual(PhaseOrder.MAKE_SUBVOL, item.phase_order())
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            new_subvol_name = "differs_from_create_ops"
            subvol = temp_subvolumes.caller_will_create(new_subvol_name)
            item.get_phase_builder([item], DUMMY_LAYER_OPTS_BA)(subvol)
            rendered_subvol = render_subvol(subvol)
            self.assertEqual(
                get_meta_dir_contents(),
                pop_path(rendered_subvol, ".meta"),
            )
            self.assertEqual(
                render_demo_subvols(
                    create_ops=new_subvol_name, lossy_packaging=lossy_packaging
                ),
                rendered_subvol,
            )

    def test_receive_sendstream(self):
        self._check_receive_package(
            LayerFromPackageItem(
                format="sendstream",
                from_target="t",
                source=Path(__file__).dirname() / "create_ops.sendstream",
            ),
        )

    def test_receive_tarball(self):
        self._check_receive_package(
            LayerFromPackageItem(
                format="tar",
                from_target="t",
                source=Path(__file__).dirname() / "create_ops.tar.gz",
            ),
            lossy_packaging="tar",
        )

    def test_unsupported_format(self):
        with self.assertRaisesRegex(Exception, "Unsupported format"):
            self._check_receive_package(
                LayerFromPackageItem(
                    format="test",
                    from_target="t",
                    source=Path(__file__).dirname() / "create_ops.tar.gz",
                ),
            )
