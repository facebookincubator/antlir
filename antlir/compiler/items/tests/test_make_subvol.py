#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import sys
import tempfile

from antlir.btrfs_diff.tests.demo_sendstreams_expected import render_demo_subvols

from antlir.compiler.items.common import PhaseOrder
from antlir.compiler.items.ensure_dirs_exist import (
    ensure_subdirs_exist_factory,
    EnsureDirsExistItem,
)
from antlir.compiler.items.make_subvol import (
    _resolve_image_source,
    FilesystemRootItem,
    LayerFromPackageItem,
    ParentLayerItem,
    ZST_EXTENSION,
)
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    get_dummy_layer_opts_ba,
    render_subvol,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.subvol_helpers import get_meta_dir_contents, pop_path


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
                    {".meta": get_meta_dir_contents(subvol)},
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
            EnsureDirsExistItem(from_target="t", into_dir="a", basename="c").build(
                child, DUMMY_LAYER_OPTS_BA
            )

            # The parent is unchanged.
            self.assertEqual(parent_content, render_subvol(parent))
            child_content = copy.deepcopy(parent_content)
            child_content[1]["a"][1]["c"] = ["(Dir)", {}]
            # Since the parent lacked a /.meta, the child added it.
            child_content[1][".meta"] = get_meta_dir_contents(child)
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
                get_meta_dir_contents(subvol),
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

    def test_unsupported_format(self):
        with self.assertRaisesRegex(Exception, "Unsupported format"):
            self._check_receive_package(
                LayerFromPackageItem(
                    format="tar",
                    from_target="t",
                    source=Path(__file__).dirname() / "create_ops.tar.gz",
                ),
            )

    def test_resolve_image_source_v1_from_v2(self):
        format = "sendstream"
        with tempfile.NamedTemporaryFile() as f:
            v1_file = Path(f.name + ZST_EXTENSION.decode("ascii"))
            v2_file = _resolve_image_source(format, v1_file)
            self.assertEqual(f.name, str(v2_file))

    def test_resolve_image_source_v2_from_v1(self):
        format = "sendstream.v2"
        with tempfile.NamedTemporaryFile(suffix=ZST_EXTENSION.decode("ascii")) as f:
            v2_file = Path(f.name[: -len(ZST_EXTENSION)])
            v1_file = _resolve_image_source(format, v2_file)
            self.assertEqual(f.name, str(v1_file))
