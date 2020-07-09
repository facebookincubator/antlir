#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
import unittest
from dataclasses import dataclass

from fs_image.compiler.items.common import ImageItem, PhaseOrder
from fs_image.compiler.items.foreign_layer import ForeignLayerItem
from fs_image.compiler.items.install_file import InstallFileItem
from fs_image.compiler.items.make_dirs import MakeDirsItem
from fs_image.compiler.items.make_subvol import FilesystemRootItem
from fs_image.compiler.items.phases_provide import PhasesProvideItem
from fs_image.compiler.items.remove_path import RemovePathItem
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..dep_graph import (
    DependencyGraph,
    ItemProv,
    ItemReq,
    ItemReqsProvs,
    ValidatedReqsProvs,
)
from ..requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    require_directory,
)


# Since the constructor of `InstallFileItem` tries to `os.stat` its input,
# we need to give it filenames that exist.
_FILE1 = "/etc/passwd"
_FILE2 = "/etc/group"
PATH_TO_ITEM = {
    "/a/b/c": MakeDirsItem(from_target="", into_dir="/", path_to_make="a/b/c"),
    "/a/d/e": MakeDirsItem(from_target="", into_dir="a", path_to_make="d/e"),
    "/a/b/c/F": InstallFileItem(from_target="", source=_FILE1, dest="a/b/c/F"),
    "/a/d/e/G": InstallFileItem(from_target="", source=_FILE2, dest="a/d/e/G"),
}


def _fs_root_phases(item):
    return [(FilesystemRootItem.get_phase_builder, (item,))]


class ValidateReqsProvsTestCase(unittest.TestCase):
    def test_duplicate_paths_in_same_item(self):
        @dataclass(init=False, frozen=True)
        class BadDuplicatePathItem(ImageItem):
            def requires(self):
                yield require_directory("a")

            def provides(self):
                yield ProvidesDirectory(path="a")

        with self.assertRaisesRegex(AssertionError, "^Same path in "):
            ValidatedReqsProvs([BadDuplicatePathItem(from_target="t")])

    def test_duplicate_paths_provided(self):
        with self.assertRaisesRegex(
            RuntimeError, "^Both .* and .* from .* provide the same path$"
        ):
            ValidatedReqsProvs(
                [
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    MakeDirsItem(
                        from_target="", into_dir="/", path_to_make="y/x"
                    ),
                ]
            )

    def test_unmatched_requirement(self):
        item = InstallFileItem(from_target="", source=_FILE1, dest="y")
        with self.assertRaises(
            RuntimeError,
            msg="^At /: nothing in set() matches the requirement "
            f'{ItemReq(requires=require_directory("/"), item=item)}$',
        ):
            ValidatedReqsProvs([item])

    def test_paths_to_reqs_provs(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("subvol")
            provides_root = PhasesProvideItem(from_target="t", subvol=subvol)
            expected = {
                "/meta": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDoNotAccess(path="/meta"), provides_root
                        )
                    },
                    item_reqs=set(),
                ),
                "/": ItemReqsProvs(
                    item_provs={
                        ItemProv(ProvidesDirectory(path="/"), provides_root)
                    },
                    item_reqs={
                        ItemReq(require_directory("/"), PATH_TO_ITEM["/a/b/c"])
                    },
                ),
                "/a": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDirectory(path="a"), PATH_TO_ITEM["/a/b/c"]
                        )
                    },
                    item_reqs={
                        ItemReq(require_directory("a"), PATH_TO_ITEM["/a/d/e"])
                    },
                ),
                "/a/b": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDirectory(path="a/b"),
                            PATH_TO_ITEM["/a/b/c"],
                        )
                    },
                    item_reqs=set(),
                ),
                "/a/b/c": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDirectory(path="a/b/c"),
                            PATH_TO_ITEM["/a/b/c"],
                        )
                    },
                    item_reqs={
                        ItemReq(
                            require_directory("a/b/c"), PATH_TO_ITEM["/a/b/c/F"]
                        )
                    },
                ),
                "/a/b/c/F": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesFile(path="a/b/c/F"),
                            PATH_TO_ITEM["/a/b/c/F"],
                        )
                    },
                    item_reqs=set(),
                ),
                "/a/d": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDirectory(path="a/d"),
                            PATH_TO_ITEM["/a/d/e"],
                        )
                    },
                    item_reqs=set(),
                ),
                "/a/d/e": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesDirectory(path="a/d/e"),
                            PATH_TO_ITEM["/a/d/e"],
                        )
                    },
                    item_reqs={
                        ItemReq(
                            require_directory("a/d/e"), PATH_TO_ITEM["/a/d/e/G"]
                        )
                    },
                ),
                "/a/d/e/G": ItemReqsProvs(
                    item_provs={
                        ItemProv(
                            ProvidesFile(path="a/d/e/G"),
                            PATH_TO_ITEM["/a/d/e/G"],
                        )
                    },
                    item_reqs=set(),
                ),
            }
            self.assertEqual(
                ValidatedReqsProvs(
                    [provides_root, *PATH_TO_ITEM.values()]
                ).path_to_reqs_provs,
                expected,
            )


class DependencyGraphTestCase(unittest.TestCase):
    def test_item_predecessors(self):
        dg = DependencyGraph(PATH_TO_ITEM.values(), layer_target="t-34")
        self.assertEqual(
            _fs_root_phases(FilesystemRootItem(from_target="t-34")),
            list(dg.ordered_phases()),
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("subvol")
            phases_provide = PhasesProvideItem(from_target="t", subvol=subvol)
            ns = dg._prep_item_predecessors(phases_provide)
        path_to_item = {"/": phases_provide, **PATH_TO_ITEM}
        self.assertEqual(
            ns.item_to_predecessors,
            {
                path_to_item[k]: {path_to_item[v] for v in vs}
                for k, vs in {
                    "/a/b/c": {"/"},
                    "/a/d/e": {"/a/b/c"},
                    "/a/b/c/F": {"/a/b/c"},
                    "/a/d/e/G": {"/a/d/e"},
                }.items()
            },
        )
        self.assertEqual(
            ns.predecessor_to_items,
            {
                path_to_item[k]: {path_to_item[v] for v in vs}
                for k, vs in {
                    "/": {"/a/b/c"},
                    "/a/b/c": {"/a/d/e", "/a/b/c/F"},
                    "/a/b/c/F": set(),
                    "/a/d/e": {"/a/d/e/G"},
                    "/a/d/e/G": set(),
                }.items()
            },
        )
        self.assertEqual(ns.items_without_predecessors, {path_to_item["/"]})

    def test_foreign_layer_assert(self):
        foreign1 = ForeignLayerItem(
            from_target="t1", cmd=["x"], user="y", serve_rpm_snapshots=()
        )
        foreign2 = ForeignLayerItem(
            from_target="t2", cmd=["a"], user="b", serve_rpm_snapshots=()
        )

        # Good path: one FOREIGN_LAYER & default MAKE_SUBVOL
        DependencyGraph([foreign1], "layer_t")

        # Too many foreign layers
        with self.assertRaises(AssertionError):
            DependencyGraph([foreign1, foreign2], "layer_t")

        # Cannot mix foreign layer & depedency-sortable item
        with self.assertRaises(AssertionError):
            DependencyGraph(
                [
                    foreign1,
                    MakeDirsItem(
                        from_target="", into_dir="a", path_to_make="b"
                    ),
                ],
                "layer_t",
            )

        # Cannot have other phase items
        with self.assertRaises(AssertionError):
            DependencyGraph(
                [
                    foreign1,
                    RemovePathItem(
                        from_target="", path="x", action="if_exists"
                    ),
                ],
                "layer_t",
            )


class DependencyOrderItemsTestCase(unittest.TestCase):
    def test_gen_dependency_graph(self):
        dg = DependencyGraph(PATH_TO_ITEM.values(), layer_target="t-72")
        self.assertEqual(
            _fs_root_phases(FilesystemRootItem(from_target="t-72")),
            list(dg.ordered_phases()),
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("subvol")
            self.assertIn(
                tuple(
                    dg.gen_dependency_order_items(
                        PhasesProvideItem(from_target="t", subvol=subvol)
                    )
                ),
                {
                    tuple(PATH_TO_ITEM[p] for p in paths)
                    for paths in [
                        # A few orders are valid, don't make the test fragile.
                        ["/a/b/c", "/a/b/c/F", "/a/d/e", "/a/d/e/G"],
                        ["/a/b/c", "/a/d/e", "/a/b/c/F", "/a/d/e/G"],
                        ["/a/b/c", "/a/d/e", "/a/d/e/G", "/a/b/c/F"],
                    ]
                },
            )

    def test_cycle_detection(self):
        def requires_provides_directory_class(requires_dir, provides_dir):
            @dataclass(init=False, frozen=True)
            class RequiresProvidesDirectory(ImageItem):
                def requires(self):
                    yield require_directory(requires_dir)

                def provides(self):
                    yield ProvidesDirectory(path=provides_dir)

            return RequiresProvidesDirectory

        # `dg_ok`: dependency-sorting will work without a cycle
        first = FilesystemRootItem(from_target="")
        second = requires_provides_directory_class("/", "a")(from_target="")
        third = MakeDirsItem(from_target="", into_dir="a", path_to_make="b/c")
        dg_ok = DependencyGraph([second, first, third], layer_target="t")
        self.assertEqual(_fs_root_phases(first), list(dg_ok.ordered_phases()))

        # `dg_bad`: changes `second` to get a cycle
        dg_bad = DependencyGraph(
            [
                requires_provides_directory_class("a/b", "a")(from_target=""),
                first,
                third,
            ],
            layer_target="t",
        )
        self.assertEqual(_fs_root_phases(first), list(dg_bad.ordered_phases()))

        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("subvol")
            provides_root = PhasesProvideItem(from_target="t", subvol=subvol)
            self.assertEqual(
                [second, third],
                list(dg_ok.gen_dependency_order_items(provides_root)),
            )
            with self.assertRaisesRegex(AssertionError, "^Cycle in "):
                list(dg_bad.gen_dependency_order_items(provides_root))

    def test_phase_order(self):
        class FakeRemovePaths:
            get_phase_builder = "kittycat"

            def phase_order(self):
                return PhaseOrder.REMOVE_PATHS

        first = FilesystemRootItem(from_target="")
        second = FakeRemovePaths()
        third = MakeDirsItem(from_target="", into_dir="/", path_to_make="a/b")
        dg = DependencyGraph([second, first, third], layer_target="t")
        self.assertEqual(
            _fs_root_phases(first)
            + [(FakeRemovePaths.get_phase_builder, (second,))],
            list(dg.ordered_phases()),
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("subvol")
            self.assertEqual(
                [third],
                list(
                    dg.gen_dependency_order_items(
                        PhasesProvideItem(from_target="t", subvol=subvol)
                    )
                ),
            )


if __name__ == "__main__":
    unittest.main()
