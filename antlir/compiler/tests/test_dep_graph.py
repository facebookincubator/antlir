#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
import unittest
from dataclasses import dataclass

from antlir.compiler.items.common import ImageItem, PhaseOrder
from antlir.compiler.items.ensure_dir_exists import (
    ensure_dir_exists_factory,
)
from antlir.compiler.items.foreign_layer import ForeignLayerItem
from antlir.compiler.items.foreign_layer_t import foreign_layer_t
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.symlink import SymlinkToDirItem
from antlir.tests.temp_subvolumes import TempSubvolumes

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


def _fs_root_phases(item):
    return [(FilesystemRootItem.get_phase_builder, (item,))]


def _build_req_prov(path, req_items, prov_items, prov_t=None):
    prov_t = ProvidesDirectory if prov_t is None else prov_t
    return ItemReqsProvs(
        item_reqs={ItemReq(require_directory(path=path), i) for i in req_items},
        item_provs={ItemProv(prov_t(path=path), i) for i in prov_items},
    )


class DepGraphTestBase(unittest.TestCase):
    def setUp(self):
        self.maxDiff = None
        unittest.util._MAX_LENGTH = 12345
        self._temp_svs_ctx = TempSubvolumes(sys.argv[0])
        temp_svs = self._temp_svs_ctx.__enter__()
        self.addCleanup(self._temp_svs_ctx.__exit__, None, None, None)
        self.provides_root = PhasesProvideItem(
            from_target="t", subvol=temp_svs.create("subvol")
        )
        abc, ab, a = list(
            ensure_dir_exists_factory(from_target="", path="/a/b/c")
        )
        ade, ad, a2 = list(
            ensure_dir_exists_factory(from_target="", path="/a/d/e")
        )
        self.assertEqual(a, a2)
        abcf = InstallFileItem(from_target="", source=_FILE1, dest="a/b/c/F")
        adeg = InstallFileItem(from_target="", source=_FILE2, dest="a/d/e/G")
        a_ln = SymlinkToDirItem(from_target="", source="/a", dest="/a/d/e")
        # There is a bit of duplication here but it's clearer to explicitly
        # define our expectations around these rather than derive them from one
        # another. It's also simpler to define these here where we have access
        # to the item variables rather than make all of those class variables
        # and define the maps in various test functions.
        self.item_to_items_it_reqs = {
            a: {self.provides_root},
            ab: {a},
            abc: {ab},
            ad: {a},
            ade: {ad, a_ln},
            abcf: {abc},
            adeg: {ade, a_ln},
            a_ln: {a, ad},
        }
        self.items = self.item_to_items_it_reqs.keys()
        self.item_to_items_it_provs = {
            self.provides_root: {a},
            a: {ab, ad, a_ln},
            ab: {abc},
            abc: {abcf},
            ad: {ade, a_ln},
            ade: {adeg},
            a_ln: {ade, adeg},
        }
        # [[items, requiring, it], [items, it, requires]]
        self.path_to_reqs_provs = {
            "/.meta": _build_req_prov(
                "/.meta", [], [self.provides_root], ProvidesDoNotAccess
            ),
            "/": _build_req_prov("/", [a], [self.provides_root]),
            "/a": _build_req_prov("/a", [ab, ad, a_ln], [a]),
            "/a/b": _build_req_prov("/a/b", [abc], [ab]),
            "/a/b/c": _build_req_prov("/a/b/c", [abcf], [abc]),
            "/a/d": _build_req_prov("/a/d", [ade, a_ln], [ad]),
            "/a/d/e": _build_req_prov("/a/d/e", [adeg], [ade, a_ln]),
            "/a/d/e/G": _build_req_prov("/a/d/e/G", [], [adeg], ProvidesFile),
            "/a/b/c/F": _build_req_prov("/a/b/c/F", [], [abcf], ProvidesFile),
        }


class ValidateReqsProvsTestCase(DepGraphTestBase):
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
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    *ensure_dir_exists_factory(from_target="", path="/y/x"),
                ]
            )

    def test_allowed_duplicate_paths(self):
        ValidatedReqsProvs(
            [
                self.provides_root,
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                *ensure_dir_exists_factory(from_target="", path="/y/x"),
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
        self.assertDictEqual(
            ValidatedReqsProvs(
                {self.provides_root, *self.items}
            ).path_to_reqs_provs,
            self.path_to_reqs_provs,
        )


class DependencyGraphTestCase(DepGraphTestBase):
    def test_item_predecessors(self):
        dg = DependencyGraph(self.items, layer_target="t-34")
        self.assertEqual(
            _fs_root_phases(FilesystemRootItem(from_target="t-34")),
            list(dg.ordered_phases()),
        )
        ns = dg._prep_item_predecessors(self.provides_root)

        self.assertDictEqual(
            ns.item_to_predecessors,
            self.item_to_items_it_reqs,
        )
        self.assertDictEqual(
            ns.predecessor_to_items,
            self.item_to_items_it_provs,
        )
        self.assertEqual(ns.items_without_predecessors, {self.provides_root})

    def test_foreign_layer_assert(self):
        foreign1 = ForeignLayerItem(
            from_target="t1",
            cmd=["x"],
            user="y",
            container_opts=foreign_layer_t.types.container_opts(),
        )
        foreign2 = ForeignLayerItem(
            from_target="t2",
            cmd=["a"],
            user="b",
            container_opts=foreign_layer_t.types.container_opts(),
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
                    *ensure_dir_exists_factory(from_target="", path="a/b"),
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


class DependencyOrderItemsTestCase(DepGraphTestBase):
    def assert_before(self, res, x, y):
        self.assertLess(res.index(x), res.index(y))

    def test_gen_dependency_graph(self):
        dg = DependencyGraph(self.items, layer_target="t-72")
        self.assertEqual(
            _fs_root_phases(FilesystemRootItem(from_target="t-72")),
            list(dg.ordered_phases()),
        )
        res = tuple(dg.gen_dependency_order_items(self.provides_root))
        self.assertNotIn(self.provides_root, res)
        res = (self.provides_root, *res)
        for item, items_it_requires in self.item_to_items_it_reqs.items():
            for item_it_requires in items_it_requires:
                self.assertLess(
                    res.index(item_it_requires),
                    res.index(item),
                    f"{item_it_requires} was not before {item}",
                )

        for item, items_requiring_it in self.item_to_items_it_provs.items():
            for item_requiring_it in items_requiring_it:
                self.assertLess(
                    res.index(item),
                    res.index(item_requiring_it),
                    f"{item} was not before {item_requiring_it}",
                )

    def test_cycle_detection(self):
        def requires_provides_directory_class(requires_dir, provides_dirs):
            @dataclass(init=False, frozen=True)
            class RequiresProvidesDirectory(ImageItem):
                def requires(self):
                    yield require_directory(requires_dir)

                def provides(self):
                    for d in provides_dirs:
                        yield ProvidesDirectory(path=d)

            return RequiresProvidesDirectory

        # `dg_ok`: dependency-sorting will work without a cycle
        first = FilesystemRootItem(from_target="")
        second = requires_provides_directory_class("/", ["a"])(from_target="")
        third = requires_provides_directory_class("/a", ["/a/b", "/a/b/c"])(
            from_target=""
        )
        dg_ok = DependencyGraph([second, first, third], layer_target="t")
        self.assertEqual(_fs_root_phases(first), list(dg_ok.ordered_phases()))

        # `dg_bad`: changes `second` to get a cycle
        dg_bad = DependencyGraph(
            [
                requires_provides_directory_class("a/b", ["a"])(from_target=""),
                first,
                third,
            ],
            layer_target="t",
        )
        self.assertEqual(_fs_root_phases(first), list(dg_bad.ordered_phases()))

        self.assertEqual(
            [second, third],
            list(dg_ok.gen_dependency_order_items(self.provides_root)),
        )
        with self.assertRaisesRegex(AssertionError, "^Cycle in "):
            list(dg_bad.gen_dependency_order_items(self.provides_root))

    def test_phase_order(self):
        class FakeRemovePaths:
            get_phase_builder = "kittycat"

            def phase_order(self):
                return PhaseOrder.REMOVE_PATHS

        first = FilesystemRootItem(from_target="")
        second = FakeRemovePaths()
        rest = list(ensure_dir_exists_factory(from_target="", path="/a/b"))[
            ::-1
        ]
        dg = DependencyGraph([second, first, *rest], layer_target="t")
        self.assertEqual(
            _fs_root_phases(first)
            + [(FakeRemovePaths.get_phase_builder, (second,))],
            list(dg.ordered_phases()),
        )
        self.assertEqual(
            rest,
            list(dg.gen_dependency_order_items(self.provides_root)),
        )


if __name__ == "__main__":
    unittest.main()
