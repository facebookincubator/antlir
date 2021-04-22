#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
import unittest
from dataclasses import dataclass
from typing import Dict, Iterator, Optional, Set, Tuple

from antlir.compiler.items.common import ImageItem, PhaseOrder
from antlir.compiler.items.ensure_dirs_exist import (
    EnsureDirsExistItem,
    ensure_subdirs_exist_factory,
)
from antlir.compiler.items.genrule_layer import GenruleLayerItem
from antlir.compiler.items.genrule_layer_t import genrule_layer_t
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes

from ..dep_graph import (
    _symlink_target_normpath,
    DependencyGraph,
    ItemProv,
    ItemReq,
    ItemReqsProvs,
    PathItemReqsProvs,
    ValidatedReqsProvs,
)
from ..requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
    ProvidesSymlink,
    RequireDirectory,
    RequireFile,
    RequireGroup,
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
        item_reqs={
            ItemReq(RequireDirectory(path=Path(path)), i) for i in req_items
        },
        item_provs={ItemProv(prov_t(path=Path(path)), i) for i in prov_items},
    )


class ItemProvTest(unittest.TestCase):
    def test_no_conflict_with_dup_ensure_dirs(self):
        self.assertFalse(
            ItemProv(
                provides=ProvidesDirectory(path=Path("/a/b")),
                item=EnsureDirsExistItem(
                    from_target="",
                    into_dir="/a",
                    basename="b",
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesDirectory(path=Path("/a/b")),
                    item=EnsureDirsExistItem(
                        from_target="",
                        into_dir="/a",
                        basename="b",
                    ),
                )
            )
        )

    def test_no_conflict_with_dup_symlink(self):
        self.assertFalse(
            ItemProv(
                provides=ProvidesDirectory(path=Path("/a/b")),
                item=SymlinkToDirItem(
                    from_target="", source="/x/y", dest="/a/b"
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesDirectory(path=Path("/a/b")),
                    item=SymlinkToDirItem(
                        from_target="", source="/x/y", dest="/a/b"
                    ),
                )
            )
        )
        self.assertFalse(
            ItemProv(
                provides=ProvidesFile(path=Path("/a/b")),
                item=SymlinkToFileItem(
                    from_target="", source="/x/y", dest="/a/b"
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesFile(path=Path("/a/b")),
                    item=SymlinkToFileItem(
                        from_target="", source="/x/y", dest="/a/b"
                    ),
                )
            )
        )

    def test_no_conflict_with_symlink_and_ensure_dirs(self):
        self.assertFalse(
            ItemProv(
                provides=ProvidesDirectory(path=Path("/a/b")),
                item=EnsureDirsExistItem(
                    from_target="",
                    into_dir="/a",
                    basename="b",
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesDirectory(path=Path("/a/b")),
                    item=SymlinkToDirItem(
                        from_target="", source="/x/y", dest="/a/b"
                    ),
                )
            )
        )
        self.assertFalse(
            ItemProv(
                provides=ProvidesDirectory(path=Path("/a/b")),
                item=SymlinkToDirItem(
                    from_target="", source="/x/y", dest="/a/b"
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesDirectory(path=Path("/a/b")),
                    item=EnsureDirsExistItem(
                        from_target="",
                        into_dir="/a",
                        basename="b",
                    ),
                )
            )
        )

    def test_conflict_with_different_items(self):
        self.assertTrue(
            ItemProv(
                provides=ProvidesFile(path=Path("/y/x")),
                item=InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
            ).conflicts(
                ItemProv(
                    provides=ProvidesFile(path=Path("/y/x")),
                    item=SymlinkToFileItem(
                        from_target="", source=_FILE1, dest="y/x"
                    ),
                )
            )
        )

    def test_conflict_with_symlink_dest_mismatch(self):
        self.assertTrue(
            ItemProv(
                provides=ProvidesFile(path=Path("/a/b")),
                item=SymlinkToFileItem(
                    from_target="", source="/x/y", dest="/a/b"
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesFile(path=Path("/a/b")),
                    item=SymlinkToFileItem(
                        from_target="", source="/x/y", dest="/d/c"
                    ),
                )
            )
        )
        self.assertTrue(
            ItemProv(
                provides=ProvidesDirectory(path=Path("/a/b")),
                item=SymlinkToDirItem(
                    from_target="", source="/x/y", dest="/a/b"
                ),
            ).conflicts(
                ItemProv(
                    provides=ProvidesDirectory(path=Path("/a/b")),
                    item=SymlinkToDirItem(
                        from_target="", source="/x/y", dest="/d/c"
                    ),
                )
            )
        )


@dataclass(frozen=True)
class TestImageItem(ImageItem):
    provs: Tuple[ItemProv]
    reqs: Tuple[ItemReq]

    def __init__(
        self,
        provs: Iterator[ItemProv] = None,
        reqs: Iterator[ItemReq] = None,
    ):
        provs = tuple(provs) if provs else ()
        reqs = tuple(reqs) if reqs else ()
        super().__init__(from_target="t", provs=provs, reqs=reqs)

    def provides(self):
        yield from self.provs

    def requires(self):
        yield from self.reqs


class ItemReqsProvsTest(unittest.TestCase):
    def test_item_self_conflict(self):
        @dataclass
        class TestCase:
            item_reqs_provs: ItemReqsProvs
            item: ImageItem
            want: bool

        req = RequireFile(path=Path("foo"))
        prov = ProvidesFile(path=Path("foo"))
        item_with_both = TestImageItem(reqs=[req], provs=[prov])
        item_with_duplicate_req = TestImageItem(reqs=[req, req])
        item_with_duplicate_prov = TestImageItem(provs=[prov, prov])

        tests = [
            TestCase(
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                item=TestImageItem(),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                item=TestImageItem(reqs=[RequireFile(path=Path("foo"))]),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                item=TestImageItem(provs=[RequireFile(path=Path("foo"))]),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[ItemProv(provides=prov, item=item_with_both)],
                    item_reqs=[],
                ),
                item=item_with_both,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[],
                    item_reqs=[ItemReq(requires=req, item=item_with_both)],
                ),
                item=item_with_both,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[],
                    item_reqs=[
                        ItemReq(requires=req, item=item_with_duplicate_req)
                    ],
                ),
                item=item_with_duplicate_req,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[
                        ItemProv(provides=prov, item=item_with_duplicate_prov)
                    ],
                    item_reqs=[],
                ),
                item=item_with_duplicate_prov,
                want=True,
            ),
            # In the next two cases, even though `TestImageItem` are `__eq__`,
            # this is *not* a "self conflict" because it's two different
            # `ImageItem` instances.
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[],
                    item_reqs=[
                        ItemReq(requires=req, item=TestImageItem(reqs=[req])),
                    ],
                ),
                item=TestImageItem(reqs=[req]),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    item_provs=[
                        ItemProv(
                            provides=prov, item=TestImageItem(provs=[prov])
                        ),
                    ],
                    item_reqs=[],
                ),
                item=TestImageItem(provs=[prov]),
                want=False,
            ),
        ]

        for i, test in enumerate(tests):
            have = test.item_reqs_provs._item_self_conflict(test.item)
            self.assertEqual(
                test.want, have, f"{i}: {test}, want={test.want} have={have}"
            )

    def test_symlink_item_prov(self):
        @dataclass
        class Test:
            item_provs: Set[ItemProv]
            want: Optional[ItemProv]

        symlink_item_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/foo"), target=Path("/bar")),
            item=None,
        )

        tests: Dict[Test] = {
            "empty": Test(item_provs=set(), want=None),
            "no symlink": Test(
                item_provs={
                    ItemProv(
                        provides=ProvidesFile(path=Path("/foo")), item=None
                    ),
                    ItemProv(
                        provides=ProvidesDirectory(path=Path("/bar")), item=None
                    ),
                },
                want=None,
            ),
            "has symlink": Test(
                item_provs={
                    ItemProv(
                        provides=ProvidesFile(path=Path("/foo")), item=None
                    ),
                    symlink_item_prov,
                    ItemProv(
                        provides=ProvidesDirectory(path=Path("/bar")), item=None
                    ),
                },
                want=symlink_item_prov,
            ),
        }

        for desc, test in tests.items():
            irps = ItemReqsProvs(item_provs=test.item_provs, item_reqs=set())
            have = irps.symlink_item_prov()
            self.assertEqual(
                have, test.want, f"{desc}: have={have}, want={test.want}"
            )


class PathItemReqsProvsTestCase(unittest.TestCase):
    def test_symlinked_dir(self):
        pirp = PathItemReqsProvs()

        prov_usr = ProvidesDirectory(path=Path("/usr"))
        prov_usr_item = TestImageItem(provs=[prov_usr])
        pirp.add_provider(prov_usr, prov_usr_item)

        prov_usr_bin = ProvidesDirectory(path=Path("/usr/bin"))
        prov_usr_bin_item = TestImageItem(provs=[prov_usr_bin])
        pirp.add_provider(prov_usr_bin, prov_usr_bin_item)

        prov_usr_bin_bash = ProvidesFile(path=Path("/usr/bin/bash"))
        prov_usr_bin_bash_item = TestImageItem(provs=[prov_usr_bin])
        pirp.add_provider(prov_usr_bin_bash, prov_usr_bin_bash_item)

        req_usr_bin = RequireDirectory(path=Path("/usr/bin"))
        prov_symlink = ProvidesSymlink(
            path=Path("/bin"), target=Path("/usr/bin")
        )
        prov_symlink_item = TestImageItem(
            reqs=[req_usr_bin],
            provs=[prov_symlink],
        )
        pirp.add_requirement(req_usr_bin, prov_symlink_item)
        pirp.add_provider(prov_symlink, prov_symlink_item)

        # this requirement needs to be fulfilled via /bin -> /usr/bin
        req_bin_bash = RequireFile(path=Path("/bin/bash"))
        req_bin_bash_item = TestImageItem(reqs=[req_bin_bash])
        pirp.add_requirement(req_bin_bash, req_bin_bash_item)

        pirp.validate()

    def test_realpath_item_provs(self):
        @dataclass
        class Test:
            path_to_item_reqs_provs: Dict[Path, ItemReqsProvs]
            path: Path
            want: Optional[Set[ItemProv]]

        foo_bar_symlink_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/foo"), target=Path("/bar")),
            item=None,
        )
        bar_prov = ItemProv(
            provides=ProvidesFile(path=Path("/bar")),
            item=None,
        )
        bar_baz_symlink_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/bar"), target=Path("baz")),
            item=None,
        )
        baz_prov = ItemProv(
            provides=ProvidesDirectory(path=Path("/baz")),
            item=None,
        )

        tests: Dict[Test] = {
            "no paths": Test(
                path_to_item_reqs_provs={}, path=Path("/foo"), want=None
            ),
            "busted symlink": Test(
                path_to_item_reqs_provs={
                    Path("/foo"): ItemReqsProvs(
                        item_provs={
                            ItemProv(
                                provides=ProvidesSymlink(
                                    path=Path("/foo"), target=Path("/missing")
                                ),
                                item=None,
                            ),
                        },
                        item_reqs=set(),
                    ),
                },
                path=Path("/foo"),
                want=None,
            ),
            "single link": Test(
                path_to_item_reqs_provs={
                    Path("/foo"): ItemReqsProvs(
                        item_provs={foo_bar_symlink_prov},
                        item_reqs=set(),
                    ),
                    Path("/bar"): ItemReqsProvs(
                        item_provs={bar_prov},
                        item_reqs=set(),
                    ),
                },
                path=Path("/foo"),
                want={foo_bar_symlink_prov, bar_prov},
            ),
            "double link": Test(
                path_to_item_reqs_provs={
                    Path("/foo"): ItemReqsProvs(
                        item_provs={foo_bar_symlink_prov},
                        item_reqs=set(),
                    ),
                    Path("/bar"): ItemReqsProvs(
                        item_provs={bar_baz_symlink_prov},
                        item_reqs=set(),
                    ),
                    Path("/baz"): ItemReqsProvs(
                        item_provs={baz_prov},
                        item_reqs=set(),
                    ),
                },
                path=Path("/foo"),
                want={foo_bar_symlink_prov, bar_baz_symlink_prov, baz_prov},
            ),
        }
        for desc, test in tests.items():
            pirp = PathItemReqsProvs()
            pirp.path_to_item_reqs_provs = test.path_to_item_reqs_provs
            have = pirp._realpath_item_provs(test.path)
            self.assertEqual(
                have, test.want, f"{desc}: have={have}, want={test.want}"
            )

    def test_realpath_item_provs_absolute(self):
        pirp = PathItemReqsProvs()
        with self.assertRaisesRegex(AssertionError, r"foo must be absolute"):
            pirp._realpath_item_provs(Path("foo"))

    def test_circular_realpath_item_provs(self):
        pirp = PathItemReqsProvs()
        pirp.path_to_item_reqs_provs = {
            Path("/a"): ItemReqsProvs(
                item_provs={
                    ItemProv(
                        provides=ProvidesSymlink(
                            path=Path("/a"),
                            target=Path("/b"),
                        ),
                        item=None,
                    ),
                },
                item_reqs={
                    ItemReq(
                        requires=RequireDirectory(path=Path("/a")),
                        item=None,
                    )
                },
            ),
            Path("/b"): ItemReqsProvs(
                item_provs={
                    ItemProv(
                        provides=ProvidesSymlink(
                            path=Path("/b"),
                            target=Path("/a"),
                        ),
                        item=None,
                    ),
                },
                item_reqs=set(),
            ),
        }
        with self.assertRaisesRegex(RuntimeError, r"^Circular realpath"):
            pirp._realpath_item_provs(Path("/a"))

    def test_symlink_target_normpath(self):
        @dataclass
        class Test:
            path: Path
            target: Path
            want: Path

        tests: Dict[str, Test] = {
            "abs target": Test(
                path=Path("/bin"),
                target=Path("/usr/bin"),
                want=Path("/usr/bin"),
            ),
            "relative target": Test(
                path=Path("/bin"),
                target=Path("../usr/bin"),
                want=Path("/usr/bin"),
            ),
            "deep abs target": Test(
                path=Path("/a/b/c"),
                target=Path("/usr/bin"),
                want=Path("/usr/bin"),
            ),
            "deep rel target": Test(
                path=Path("/a/b/c/d"), target=Path("../../e"), want=Path("/a/e")
            ),
            "same dir target": Test(
                path=Path("/a/b"), target=Path("c"), want=Path("/a/c")
            ),
            "multiple relatives": Test(
                path=Path("/a/b/c/d"),
                target=Path("../c/../../b/e"),
                want=Path("/a/b/e"),
            ),
        }
        assert len(tests) == 6
        for desc, test in tests.items():
            have = _symlink_target_normpath(test.path, test.target)
            self.assertEqual(
                have, test.want, f"{desc}: have={have}, want={test.want}"
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
            ensure_subdirs_exist_factory(
                from_target="", into_dir="/", subdirs_to_create="/a/b/c"
            )
        )
        ade, ad = list(
            ensure_subdirs_exist_factory(
                from_target="", into_dir="a", subdirs_to_create="d/e"
            )
        )
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
        item_reqs_provs = [
            _build_req_prov(
                "/.meta", [], [self.provides_root], ProvidesDoNotAccess
            ),
            _build_req_prov("/", [a], [self.provides_root]),
            _build_req_prov("/a", [ab, ad, a_ln], [a]),
            _build_req_prov("/a/b", [abc], [ab]),
            _build_req_prov("/a/b/c", [abcf], [abc]),
            _build_req_prov("/a/d", [ade, a_ln], [ad]),
            _build_req_prov("/a/d/e", [adeg], [ade, a_ln]),
            _build_req_prov("/a/d/e/G", [], [adeg], ProvidesFile),
            _build_req_prov("/a/b/c/F", [], [abcf], ProvidesFile),
        ]
        self.item_reqs_provs = {}
        for irp in item_reqs_provs:
            req = {ir.requires for ir in irp.item_reqs}.union(
                {ip.provides.req for ip in irp.item_provs}
            )
            assert len(req) == 1
            self.item_reqs_provs[req.pop().path] = irp


class ValidateReqsProvsTestCase(DepGraphTestBase):
    def test_duplicate_paths_in_same_item(self):
        with self.assertRaisesRegex(
            RuntimeError,
            r"ProvidesDirectory.*TestImageItem.*conflicts in",
        ):
            ValidatedReqsProvs(
                [
                    TestImageItem(
                        reqs=[RequireDirectory(path=Path("a"))],
                        provs=[ProvidesDirectory(path=Path("a"))],
                    ),
                ],
            )

    def test_duplicate_requires_in_same_item(self):
        with self.assertRaisesRegex(
            RuntimeError,
            r"RequireDirectory.*TestImageItem.*conflicts in",
        ):
            ValidatedReqsProvs(
                [
                    TestImageItem(
                        reqs=[
                            RequireDirectory(path=Path("a")),
                            RequireDirectory(path=Path("a")),
                        ],
                    ),
                ],
            )

    def test_duplicate_paths_provided_different_types(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    *ensure_subdirs_exist_factory(
                        from_target="", into_dir="/", subdirs_to_create="/y/x"
                    ),
                ]
            )

    def test_duplicate_paths_provided(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    SymlinkToFileItem(from_target="", source="a", dest="y/x"),
                ]
            )

    def test_path_provided_twice(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y"),
                    InstallFileItem(from_target="", source=_FILE1, dest="y"),
                ]
            )

    def test_duplicate_symlink_paths_provided(self):
        ValidatedReqsProvs(
            [
                self.provides_root,
                InstallFileItem(from_target="", source=_FILE1, dest="a"),
                SymlinkToFileItem(from_target="", source="a", dest="x"),
                SymlinkToFileItem(from_target="", source="a", dest="x"),
            ]
        )
        ValidatedReqsProvs(
            [
                self.provides_root,
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                *ensure_subdirs_exist_factory(
                    from_target="", into_dir="/", subdirs_to_create="y/x"
                ),
            ]
        )

    def test_duplicate_symlink_paths_different_sources(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="a"),
                    InstallFileItem(from_target="", source=_FILE1, dest="b"),
                    SymlinkToFileItem(from_target="", source="a", dest="x"),
                    SymlinkToFileItem(from_target="", source="b", dest="x"),
                ]
            )

    def test_duplicate_symlink_file_and_dir_conflict(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                [
                    self.provides_root,
                    *ensure_subdirs_exist_factory(
                        from_target="", into_dir="/", subdirs_to_create="y/x"
                    ),
                    SymlinkToDirItem(from_target="", source="/y", dest="z"),
                    InstallFileItem(from_target="", source=_FILE1, dest="b"),
                    SymlinkToFileItem(from_target="", source="b", dest="z"),
                ]
            )

    def test_allowed_duplicate_paths(self):
        ValidatedReqsProvs(
            [
                self.provides_root,
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                *ensure_subdirs_exist_factory(
                    from_target="", into_dir="/", subdirs_to_create="y/x"
                ),
            ]
        )

    def test_non_path_requirements(self):
        ValidatedReqsProvs(
            [
                TestImageItem(provs=[ProvidesGroup(groupname="adm")]),
                TestImageItem(reqs=[RequireGroup(name="adm")]),
            ],
        )

        with self.assertRaisesRegex(
            RuntimeError,
            r"set\(\) does not provide ItemReq\(requires=RequireGroup",
        ):
            ValidatedReqsProvs([TestImageItem(reqs=[RequireGroup(name="adm")])])

    def test_unmatched_requirement(self):
        item = InstallFileItem(from_target="", source=_FILE1, dest="y")
        with self.assertRaises(
            RuntimeError,
            msg="^At /: nothing in set() matches the requirement "
            f'{ItemReq(requires=RequireDirectory(path=Path("/")), item=item)}$',
        ):
            ValidatedReqsProvs([item])

    def test_paths_to_reqs_provs(self):
        self.assertDictEqual(
            ValidatedReqsProvs(
                {self.provides_root, *self.items}
            )._path_item_reqs_provs.path_to_item_reqs_provs,
            self.item_reqs_provs,
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

    def test_genrule_layer_assert(self):
        genrule1 = GenruleLayerItem(
            from_target="t1",
            cmd=["x"],
            user="y",
            container_opts=genrule_layer_t.types.container_opts(),
        )
        genrule2 = GenruleLayerItem(
            from_target="t2",
            cmd=["a"],
            user="b",
            container_opts=genrule_layer_t.types.container_opts(),
        )

        # Good path: one GENRULE_LAYER & default MAKE_SUBVOL
        DependencyGraph([genrule1], "layer_t")

        # Too many genrule layers
        with self.assertRaises(AssertionError):
            DependencyGraph([genrule1, genrule2], "layer_t")

        # Cannot mix genrule layer & depedency-sortable item
        with self.assertRaises(AssertionError):
            DependencyGraph(
                [
                    genrule1,
                    *ensure_subdirs_exist_factory(
                        from_target="", into_dir="/", subdirs_to_create="a/b"
                    ),
                ],
                "layer_t",
            )

        # Cannot have other phase items
        with self.assertRaises(AssertionError):
            DependencyGraph(
                [
                    genrule1,
                    RemovePathItem(from_target="", path="x", must_exist=False),
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
            return TestImageItem(
                reqs=[RequireDirectory(path=Path(requires_dir))],
                provs=[ProvidesDirectory(path=Path(d)) for d in provides_dirs],
            )

        # `dg_ok`: dependency-sorting will work without a cycle
        first = FilesystemRootItem(from_target="")
        second = requires_provides_directory_class("/", ["a"])
        third = requires_provides_directory_class("/a", ["/a/b", "/a/b/c"])
        dg_ok = DependencyGraph([second, first, third], layer_target="t")
        self.assertEqual(_fs_root_phases(first), list(dg_ok.ordered_phases()))

        # `dg_bad`: changes `second` to get a cycle
        dg_bad = DependencyGraph(
            [
                requires_provides_directory_class("a/b", ["a"]),
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
        rest = list(
            ensure_subdirs_exist_factory(
                from_target="", into_dir="/", subdirs_to_create="a/b"
            )
        )[::-1]
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
