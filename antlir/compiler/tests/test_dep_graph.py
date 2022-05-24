#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import itertools
import sys
import unittest
import unittest.mock
from dataclasses import dataclass
from typing import Dict, Iterator, Optional, Set, Tuple

from antlir.bzl.genrule_layer import genrule_layer_t
from antlir.compiler.items.common import ImageItem, PhaseOrder
from antlir.compiler.items.ensure_dirs_exist import (
    ensure_subdirs_exist_factory,
    EnsureDirsExistItem,
)
from antlir.compiler.items.genrule_layer import GenruleLayerItem
from antlir.compiler.items.group import GroupItem
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from antlir.compiler.items.user import UserItem
from antlir.errors import UserError
from antlir.fs_utils import Path, temp_dir
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
    RequireSymlink,
)


# Since the constructor of `InstallFileItem` tries to `os.stat` its input,
# we need to give it filenames that exist.
_FILE1 = "/etc/passwd"
_FILE2 = "/etc/group"


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def _fs_root_phases(item, layer_target=None):
    return [
        (FilesystemRootItem.get_phase_builder, (item,)),
    ]


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def _build_req_prov(path, req_items, prov_items, prov_t=None):
    prov_t = ProvidesDirectory if prov_t is None else prov_t
    return ItemReqsProvs(
        item_reqs={
            ItemReq(RequireDirectory(path=Path(path)), i) for i in req_items
        },
        item_provs={ItemProv(prov_t(path=Path(path)), i) for i in prov_items},
    )


class ItemProvTest(unittest.TestCase):
    def test_no_conflict_with_dup_ensure_dirs(self) -> None:
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

    def test_no_conflict_with_dup_symlink(self) -> None:
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

    def test_no_conflict_with_symlink_and_ensure_dirs(self) -> None:
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

    def test_conflict_with_different_items(self) -> None:
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

    def test_conflict_with_symlink_dest_mismatch(self) -> None:
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
# pyre-fixme[13]: Attribute `provs` is never initialized.
# pyre-fixme[13]: Attribute `reqs` is never initialized.
class TestImageItem(ImageItem):
    provs: Tuple[ItemProv]
    reqs: Tuple[ItemReq]

    def __init__(
        self,
        # pyre-fixme[9]: provs has type `Iterator[ItemProv]`; used as `None`.
        provs: Iterator[ItemProv] = None,
        # pyre-fixme[9]: reqs has type `Iterator[ItemReq]`; used as `None`.
        reqs: Iterator[ItemReq] = None,
    ) -> None:
        # pyre-fixme[9]: provs has type `Iterator[ItemProv]`; used as
        #  `Union[Tuple[], typing.Tuple[ItemProv, ...]]`.
        provs = tuple(provs) if provs else ()
        # pyre-fixme[9]: reqs has type `Iterator[ItemReq]`; used as
        #  `Union[Tuple[], typing.Tuple[ItemReq, ...]]`.
        reqs = tuple(reqs) if reqs else ()
        super().__init__(from_target="t", provs=provs, reqs=reqs)

    # pyre-fixme[3]: Return type must be annotated.
    def provides(self):
        yield from self.provs

    # pyre-fixme[3]: Return type must be annotated.
    def requires(self):
        yield from self.reqs


class ItemReqsProvsTest(unittest.TestCase):
    def test_item_self_conflict(self) -> None:
        @dataclass
        class TestCase:
            item_reqs_provs: ItemReqsProvs
            item: ImageItem
            want: bool

        req = RequireFile(path=Path("foo"))
        prov = ProvidesFile(path=Path("foo"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireFile]`.
        # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesFile]`.
        item_with_both = TestImageItem(reqs=[req], provs=[prov])
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireFile]`.
        item_with_duplicate_req = TestImageItem(reqs=[req, req])
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesFile]`.
        item_with_duplicate_prov = TestImageItem(provs=[prov, prov])

        tests = [
            TestCase(
                # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                #  `List[Variable[_T]]`.
                # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                #  `List[Variable[_T]]`.
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                item=TestImageItem(),
                want=False,
            ),
            TestCase(
                # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                #  `List[Variable[_T]]`.
                # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                #  `List[Variable[_T]]`.
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
                #  `List[RequireFile]`.
                item=TestImageItem(reqs=[RequireFile(path=Path("foo"))]),
                want=False,
            ),
            TestCase(
                # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                #  `List[Variable[_T]]`.
                # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                #  `List[Variable[_T]]`.
                item_reqs_provs=ItemReqsProvs(item_provs=[], item_reqs=[]),
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
                #  `List[RequireFile]`.
                item=TestImageItem(provs=[RequireFile(path=Path("foo"))]),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[ItemProv]`.
                    item_provs=[ItemProv(provides=prov, item=item_with_both)],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[Variable[_T]]`.
                    item_reqs=[],
                ),
                item=item_with_both,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[Variable[_T]]`.
                    item_provs=[],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[ItemReq]`.
                    item_reqs=[ItemReq(requires=req, item=item_with_both)],
                ),
                item=item_with_both,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[Variable[_T]]`.
                    item_provs=[],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[ItemReq]`.
                    item_reqs=[
                        ItemReq(requires=req, item=item_with_duplicate_req)
                    ],
                ),
                item=item_with_duplicate_req,
                want=True,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[ItemProv]`.
                    item_provs=[
                        ItemProv(provides=prov, item=item_with_duplicate_prov)
                    ],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[Variable[_T]]`.
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
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[Variable[_T]]`.
                    item_provs=[],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[ItemReq]`.
                    item_reqs=[
                        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]`
                        #  but got `List[RequireFile]`.
                        ItemReq(requires=req, item=TestImageItem(reqs=[req])),
                    ],
                ),
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
                #  `List[RequireFile]`.
                item=TestImageItem(reqs=[req]),
                want=False,
            ),
            TestCase(
                item_reqs_provs=ItemReqsProvs(
                    # pyre-fixme[6]: For 1st param expected `Set[ItemProv]` but got
                    #  `List[ItemProv]`.
                    item_provs=[
                        ItemProv(
                            provides=prov,
                            # pyre-fixme[6]: For 1st param expected
                            #  `Iterator[ItemProv]` but got `List[ProvidesFile]`.
                            item=TestImageItem(provs=[prov]),
                        ),
                    ],
                    # pyre-fixme[6]: For 2nd param expected `Set[ItemReq]` but got
                    #  `List[Variable[_T]]`.
                    item_reqs=[],
                ),
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
                #  `List[ProvidesFile]`.
                item=TestImageItem(provs=[prov]),
                want=False,
            ),
        ]

        for i, test in enumerate(tests):
            have = test.item_reqs_provs._item_self_conflict(test.item)
            self.assertEqual(
                test.want, have, f"{i}: {test}, want={test.want} have={have}"
            )

    def test_symlink_item_prov(self) -> None:
        @dataclass
        class Test:
            item_provs: Set[ItemProv]
            want: Optional[ItemProv]

        symlink_item_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/foo"), target=Path("/bar")),
            # pyre-fixme[6]: For 2nd param expected `ImageItem` but got `None`.
            item=None,
        )

        # pyre-fixme[24]: Generic type `dict` expects 2 type parameters, received 1,
        #  use `typing.Dict` to avoid runtime subscripting errors.
        tests: Dict[Test] = {
            "empty": Test(item_provs=set(), want=None),
            "no symlink": Test(
                item_provs={
                    ItemProv(
                        provides=ProvidesFile(path=Path("/foo")),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
                    ),
                    ItemProv(
                        provides=ProvidesDirectory(path=Path("/bar")),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
                    ),
                },
                want=None,
            ),
            "has symlink": Test(
                item_provs={
                    ItemProv(
                        provides=ProvidesFile(path=Path("/foo")),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
                    ),
                    symlink_item_prov,
                    ItemProv(
                        provides=ProvidesDirectory(path=Path("/bar")),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
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
    def test_symlinked_dir(self) -> None:
        pirp = PathItemReqsProvs()

        prov_usr = ProvidesDirectory(path=Path("/usr"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesDirectory]`.
        prov_usr_item = TestImageItem(provs=[prov_usr])
        pirp.add_provider(prov_usr, prov_usr_item)

        prov_usr_bin = ProvidesDirectory(path=Path("/usr/bin"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesDirectory]`.
        prov_usr_bin_item = TestImageItem(provs=[prov_usr_bin])
        pirp.add_provider(prov_usr_bin, prov_usr_bin_item)

        prov_usr_bin_bash = ProvidesFile(path=Path("/usr/bin/bash"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesDirectory]`.
        prov_usr_bin_bash_item = TestImageItem(provs=[prov_usr_bin])
        pirp.add_provider(prov_usr_bin_bash, prov_usr_bin_bash_item)

        req_usr_bin = RequireDirectory(path=Path("/usr/bin"))
        prov_symlink = ProvidesSymlink(
            path=Path("/bin"), target=Path("/usr/bin")
        )
        prov_symlink_item = TestImageItem(
            # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
            #  `List[RequireDirectory]`.
            reqs=[req_usr_bin],
            # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
            #  `List[ProvidesSymlink]`.
            provs=[prov_symlink],
        )
        pirp.add_requirement(req_usr_bin, prov_symlink_item)
        pirp.add_provider(prov_symlink, prov_symlink_item)

        # this requirement needs to be fulfilled via /bin -> /usr/bin
        req_bin_bash = RequireFile(path=Path("/bin/bash"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireFile]`.
        req_bin_bash_item = TestImageItem(reqs=[req_bin_bash])
        pirp.add_requirement(req_bin_bash, req_bin_bash_item)
        pirp.validate()

        # also make sure directly requiring the symlink works
        req_symlink = RequireSymlink(path=Path("/bin"), target=Path("/usr/bin"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireSymlink]`.
        req_symlink_item = TestImageItem(reqs=[req_symlink])
        pirp.add_requirement(req_symlink, req_symlink_item)
        pirp.validate()

    def test_symlinked_dir_does_not_satisfy_require_file(self) -> None:
        pirp = PathItemReqsProvs()

        prov_a = ProvidesDirectory(path=Path("/a"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesDirectory]`.
        prov_a_item = TestImageItem(provs=[prov_a])
        pirp.add_provider(prov_a, prov_a_item)

        req_a = RequireDirectory(path=Path("/a"))
        prov_symlink = ProvidesSymlink(path=Path("/b"), target=Path("/a"))
        prov_symlink_item = TestImageItem(
            # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
            #  `List[RequireDirectory]`.
            reqs=[req_a],
            # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
            #  `List[ProvidesSymlink]`.
            provs=[prov_symlink],
        )
        pirp.add_requirement(req_a, prov_symlink_item)
        pirp.add_provider(prov_symlink, prov_symlink_item)

        # this requires a file, but the symlink is pointing to a dir
        req_b = RequireFile(path=Path("/b"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireFile]`.
        req_b_item = TestImageItem(reqs=[req_b])
        pirp.add_requirement(req_b, req_b_item)

        with self.assertRaisesRegex(UserError, r"/b: .* does not provide .*$"):
            pirp.validate()

    def test_symlinked_file_does_not_satisfy_require_dir(self) -> None:
        pirp = PathItemReqsProvs()

        prov_a = ProvidesFile(path=Path("/a"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesFile]`.
        prov_a_item = TestImageItem(provs=[prov_a])
        pirp.add_provider(prov_a, prov_a_item)

        req_a = RequireFile(path=Path("/a"))
        prov_symlink = ProvidesSymlink(path=Path("/b"), target=Path("/a"))
        prov_symlink_item = TestImageItem(
            # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
            #  `List[RequireFile]`.
            reqs=[req_a],
            # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
            #  `List[ProvidesSymlink]`.
            provs=[prov_symlink],
        )
        pirp.add_requirement(req_a, prov_symlink_item)
        pirp.add_provider(prov_symlink, prov_symlink_item)

        # this requires a dir, but the symlink is pointing to a file
        req_b = RequireDirectory(path=Path("/b"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireDirectory]`.
        req_b_item = TestImageItem(reqs=[req_b])
        pirp.add_requirement(req_b, req_b_item)

        with self.assertRaisesRegex(UserError, r"/b: .* does not provide .*$"):
            pirp.validate()

    def test_requires_symlink_explicitly_satisfied(self) -> None:
        pirp = PathItemReqsProvs()

        prov_a = ProvidesFile(path=Path("/a"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
        #  `List[ProvidesFile]`.
        prov_a_item = TestImageItem(provs=[prov_a])
        pirp.add_provider(prov_a, prov_a_item)

        prov_symlink = ProvidesSymlink(path=Path("/b"), target=Path("/foo"))
        prov_symlink_item = TestImageItem(
            # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
            #  `List[Variable[_T]]`.
            reqs=[],
            # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
            #  `List[ProvidesSymlink]`.
            provs=[prov_symlink],
        )
        pirp.add_provider(prov_symlink, prov_symlink_item)

        # this requires a dir, but the symlink is pointing to a file
        req_b = RequireSymlink(path=Path("/b"), target=Path("/bar"))
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireSymlink]`.
        req_b_item = TestImageItem(reqs=[req_b])
        pirp.add_requirement(req_b, req_b_item)

        with self.assertRaisesRegex(
            UserError,
            r"/b: .* does not provide .*; "
            r"RequireSymlink must be explicitly fulfilled$",
        ):
            pirp.validate()

    def test_realpath_item_provs(self) -> None:
        @dataclass
        class Test:
            path_to_item_reqs_provs: Dict[Path, ItemReqsProvs]
            path: Path
            want: Optional[Set[ItemProv]]

        foo_bar_symlink_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/foo"), target=Path("/bar")),
            # pyre-fixme[6]: For 2nd param expected `ImageItem` but got `None`.
            item=None,
        )
        bar_prov = ItemProv(
            provides=ProvidesFile(path=Path("/bar")),
            # pyre-fixme[6]: For 2nd param expected `ImageItem` but got `None`.
            item=None,
        )
        bar_baz_symlink_prov = ItemProv(
            provides=ProvidesSymlink(path=Path("/bar"), target=Path("baz")),
            # pyre-fixme[6]: For 2nd param expected `ImageItem` but got `None`.
            item=None,
        )
        baz_prov = ItemProv(
            provides=ProvidesDirectory(path=Path("/baz")),
            # pyre-fixme[6]: For 2nd param expected `ImageItem` but got `None`.
            item=None,
        )

        # pyre-fixme[24]: Generic type `dict` expects 2 type parameters, received 1,
        #  use `typing.Dict` to avoid runtime subscripting errors.
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
                                # pyre-fixme[6]: For 2nd param expected `ImageItem`
                                #  but got `None`.
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

    def test_realpath_item_provs_absolute(self) -> None:
        pirp = PathItemReqsProvs()
        with self.assertRaisesRegex(AssertionError, r"foo must be absolute"):
            pirp._realpath_item_provs(Path("foo"))

    def test_circular_realpath_item_provs(self) -> None:
        pirp = PathItemReqsProvs()
        pirp.path_to_item_reqs_provs = {
            Path("/a"): ItemReqsProvs(
                item_provs={
                    ItemProv(
                        provides=ProvidesSymlink(
                            path=Path("/a"),
                            target=Path("/b"),
                        ),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
                    ),
                },
                item_reqs={
                    ItemReq(
                        requires=RequireDirectory(path=Path("/a")),
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
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
                        # pyre-fixme[6]: For 2nd param expected `ImageItem` but got
                        #  `None`.
                        item=None,
                    ),
                },
                item_reqs=set(),
            ),
        }
        with self.assertRaisesRegex(RuntimeError, r"^Circular realpath"):
            pirp._realpath_item_provs(Path("/a"))

    def test_symlink_target_normpath(self) -> None:
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
    def setUp(self) -> None:
        self.maxDiff = None
        unittest.util._MAX_LENGTH = 12345
        self._temp_svs_ctx = TempSubvolumes(Path(sys.argv[0]))
        temp_svs = self._temp_svs_ctx.__enter__()
        self.addCleanup(self._temp_svs_ctx.__exit__, None, None, None)
        self._temp_dir_ctx = temp_dir()
        tmp_dir = self._temp_dir_ctx.__enter__()
        self.addCleanup(self._temp_dir_ctx.__exit__, None, None, None)
        self.provides_root = PhasesProvideItem(
            from_target="", subvol=temp_svs.create("subvol")
        )
        self.root_u = UserItem(
            from_target="",
            name="root",
            primary_group="root",
            supplementary_groups=[],
            shell="",
            home_dir="",
        )
        self.root_g = GroupItem(from_target="", name="root")
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
        abcf_src = tmp_dir / "abcf"
        abcf_src.touch()
        abcf = InstallFileItem(from_target="", source=abcf_src, dest="a/b/c/F")
        abeg_src = tmp_dir / "abeg"
        abeg_src.touch()

        adeg = InstallFileItem(from_target="", source=abeg_src, dest="a/d/e/G")
        a_ln = SymlinkToDirItem(from_target="", source="/a", dest="/a/d/e")
        # There is a bit of duplication here but it's clearer to explicitly
        # define our expectations around these rather than derive them from one
        # another. It's also simpler to define these here where we have access
        # to the item variables rather than make all of those class variables
        # and define the maps in various test functions.
        self.item_to_items_it_reqs = {
            a: {self.provides_root},
            ab: {self.provides_root, a},
            abc: {self.provides_root, ab},
            ad: {a, self.provides_root},
            ade: {ad, a_ln, self.provides_root},
            abcf: {abc, self.provides_root},
            adeg: {ade, a_ln, self.provides_root},
            a_ln: {a, ad},
        }
        self.items = self.item_to_items_it_reqs.keys()
        self.item_to_items_it_provs = {
            self.provides_root: {a, ab, abc, abcf, ad, ade, adeg},
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
            _build_req_prov("/a/d/e", [adeg], [ade]),
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
        self.item_reqs_provs[b"/a/d/e"].add_item_prov(
            ProvidesSymlink(path=Path(a_ln.dest), target=Path(a_ln.source)),
            a_ln,
        )


class ValidateReqsProvsTestCase(DepGraphTestBase):
    def test_duplicate_paths_in_same_item(self) -> None:
        with self.assertRaisesRegex(
            UserError,
            r"ProvidesDirectory.*TestImageItem.*conflicts in",
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[TestImageItem]`.
                [
                    TestImageItem(
                        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]`
                        #  but got `List[RequireDirectory]`.
                        reqs=[RequireDirectory(path=Path("a"))],
                        # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]`
                        #  but got `List[ProvidesDirectory]`.
                        provs=[ProvidesDirectory(path=Path("a"))],
                    ),
                ],
            )

    def test_duplicate_requires_in_same_item(self) -> None:
        with self.assertRaisesRegex(
            UserError,
            r"RequireDirectory.*TestImageItem.*conflicts in",
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[TestImageItem]`.
                [
                    TestImageItem(
                        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]`
                        #  but got `List[RequireDirectory]`.
                        reqs=[
                            RequireDirectory(path=Path("a")),
                            RequireDirectory(path=Path("a")),
                        ],
                    ),
                ],
            )

    def test_duplicate_paths_provided_different_types(self) -> None:
        with self.assertRaisesRegex(
            UserError, r"ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[Union[EnsureDirsExistItem, InstallFileItem,
                #  PhasesProvideItem]]`.
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    *ensure_subdirs_exist_factory(
                        from_target="", into_dir="/", subdirs_to_create="/y/x"
                    ),
                ]
            )

    def test_duplicate_paths_provided(self) -> None:
        with self.assertRaisesRegex(
            UserError, r"ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[Union[InstallFileItem, PhasesProvideItem,
                #  SymlinkToFileItem]]`.
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y/x"),
                    SymlinkToFileItem(from_target="", source="a", dest="y/x"),
                ]
            )

    def test_path_provided_twice(self) -> None:
        with self.assertRaisesRegex(
            UserError, r"ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[Union[InstallFileItem, PhasesProvideItem]]`.
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="y"),
                    InstallFileItem(from_target="", source=_FILE1, dest="y"),
                ]
            )

    def test_duplicate_symlink_paths_provided(self) -> None:
        ValidatedReqsProvs(
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[Union[InstallFileItem, PhasesProvideItem, SymlinkToFileItem]]`.
            [
                self.provides_root,
                InstallFileItem(from_target="", source=_FILE1, dest="a"),
                SymlinkToFileItem(from_target="", source="a", dest="x"),
                SymlinkToFileItem(from_target="", source="a", dest="x"),
            ]
        )
        ValidatedReqsProvs(
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[Union[EnsureDirsExistItem, PhasesProvideItem, SymlinkToDirItem]]`.
            [
                self.provides_root,
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                *ensure_subdirs_exist_factory(
                    from_target="", into_dir="/", subdirs_to_create="y/x"
                ),
            ]
        )

    def test_duplicate_symlink_paths_different_sources(self) -> None:
        with self.assertRaisesRegex(
            UserError, r"ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[Union[InstallFileItem, PhasesProvideItem,
                #  SymlinkToFileItem]]`.
                [
                    self.provides_root,
                    InstallFileItem(from_target="", source=_FILE1, dest="a"),
                    InstallFileItem(from_target="", source=_FILE1, dest="b"),
                    SymlinkToFileItem(from_target="", source="a", dest="x"),
                    SymlinkToFileItem(from_target="", source="b", dest="x"),
                ]
            )

    def test_duplicate_symlink_file_and_dir_conflict(self) -> None:
        with self.assertRaisesRegex(
            UserError, r"ItemProv.*conflicts with ItemProv"
        ):
            ValidatedReqsProvs(
                # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
                #  `List[Union[EnsureDirsExistItem, InstallFileItem, PhasesProvideItem,
                #  SymlinkToDirItem, SymlinkToFileItem]]`.
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

    def test_allowed_duplicate_paths(self) -> None:
        ValidatedReqsProvs(
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[Union[EnsureDirsExistItem, PhasesProvideItem, SymlinkToDirItem]]`.
            [
                self.provides_root,
                SymlinkToDirItem(from_target="", source="/y", dest="/y/x/z"),
                *ensure_subdirs_exist_factory(
                    from_target="", into_dir="/", subdirs_to_create="y/x"
                ),
            ]
        )

    def test_non_path_requirements(self) -> None:
        ValidatedReqsProvs(
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[TestImageItem]`.
            [
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemProv]` but got
                #  `List[ProvidesGroup]`.
                TestImageItem(provs=[ProvidesGroup(groupname="adm")]),
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
                #  `List[RequireGroup]`.
                TestImageItem(reqs=[RequireGroup(name="adm")]),
            ],
        )

        with self.assertRaisesRegex(
            UserError,
            r"set\(\) does not provide ItemReq\(requires=RequireGroup",
        ):
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[TestImageItem]`.
            # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
            #  `List[RequireGroup]`.
            ValidatedReqsProvs([TestImageItem(reqs=[RequireGroup(name="adm")])])

    def test_unmatched_requirement(self) -> None:
        item = InstallFileItem(from_target="", source=_FILE1, dest="y")
        with self.assertRaises(
            UserError,
            msg="At /: nothing in set() matches the requirement "
            f'{ItemReq(requires=RequireDirectory(path=Path("/")), item=item)}$',
        ):
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[InstallFileItem]`.
            ValidatedReqsProvs([item])

    def test_symlinked_dir(self) -> None:
        usrbin, usr = list(
            ensure_subdirs_exist_factory(
                from_target="t", into_dir="/", subdirs_to_create="usr/bin"
            )
        )
        bash = InstallFileItem(
            from_target="t", source=_FILE1, dest="usr/bin/bash"
        )
        symlink = SymlinkToDirItem(
            from_target="t", source="/usr/bin", dest="/bin"
        )
        # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
        #  `List[RequireFile]`.
        test_item = TestImageItem(reqs=[RequireFile(path=Path("/bin/bash"))])

        ValidatedReqsProvs(
            # pyre-fixme[6]: For 1st param expected `Set[ImageItem]` but got
            #  `List[Union[EnsureDirsExistItem, InstallFileItem, PhasesProvideItem,
            #  SymlinkToDirItem, TestImageItem]]`.
            [self.provides_root, usrbin, usr, bash, symlink, test_item]
        )

    def test_paths_to_reqs_provs(self) -> None:
        self.assertDictEqual(
            ValidatedReqsProvs(
                {self.provides_root, *self.items}
            )._path_item_reqs_provs.path_to_item_reqs_provs,
            self.item_reqs_provs,
        )


class DependencyGraphTestCase(DepGraphTestBase):
    def test_item_predecessors(self) -> None:
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

    def test_genrule_layer_assert(self) -> None:
        genrule1 = GenruleLayerItem(
            from_target="t1",
            cmd=["x"],
            user="y",
            # pyre-fixme[16]: `genrule_layer_t` has no attribute `types`.
            container_opts=genrule_layer_t.types.container_opts(),
        )
        genrule2 = GenruleLayerItem(
            from_target="t2",
            cmd=["a"],
            user="b",
            container_opts=genrule_layer_t.types.container_opts(),
        )

        # Good path: one GENRULE_LAYER & default MAKE_SUBVOL
        # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
        #  `List[GenruleLayerItem]`.
        DependencyGraph([genrule1], "layer_t")

        # Too many genrule layers
        with self.assertRaises(AssertionError):
            # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
            #  `List[GenruleLayerItem]`.
            DependencyGraph([genrule1, genrule2], "layer_t")

        # Cannot mix genrule layer & depedency-sortable item
        with self.assertRaises(AssertionError):
            DependencyGraph(
                # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but
                #  got `List[Union[EnsureDirsExistItem, GenruleLayerItem]]`.
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
                # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but
                #  got `List[Union[GenruleLayerItem, RemovePathItem]]`.
                [
                    genrule1,
                    RemovePathItem(from_target="", path="x", must_exist=False),
                ],
                "layer_t",
            )


class DependencyOrderItemsTestCase(DepGraphTestBase):
    # pyre-fixme[2]: Parameter must be annotated.
    def assert_before(self, res, x, y) -> None:
        self.assertLess(res.index(x), res.index(y))

    def test_skip_phases_provide(self) -> None:
        dg = DependencyGraph(
            # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
            #  `List[FilesystemRootItem]`.
            [FilesystemRootItem(from_target="t-55")],
            layer_target="t-34",
        )
        mock_pp = unittest.mock.MagicMock()
        self.assertEqual([], list(dg.gen_dependency_order_items(mock_pp)))
        mock_pp.provides.assert_not_called()

    def test_gen_dependency_graph(self) -> None:
        dg = DependencyGraph(self.items, layer_target="t-72")
        self.assertEqual(
            _fs_root_phases(FilesystemRootItem(from_target="t-72")),
            list(dg.ordered_phases()),
        )
        res = tuple(dg.gen_dependency_order_items(self.provides_root))
        self.assertNotIn(self.provides_root, res)
        res = (
            self.provides_root,
            self.root_u,
            self.root_g,
            # serialize items for the tests that check dependency order
            *itertools.chain(*res),
        )

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

    def test_cycle_detection(self) -> None:
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def requires_provides_directory_class(requires_dir, provides_dirs):
            return TestImageItem(
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
                #  `List[RequireDirectory]`.
                reqs=[RequireDirectory(path=Path(requires_dir))],
                # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
                #  `List[ProvidesDirectory]`.
                provs=[ProvidesDirectory(path=Path(d)) for d in provides_dirs],
            )

        # `dg_ok`: dependency-sorting will work without a cycle
        first = FilesystemRootItem(from_target="")
        second = requires_provides_directory_class("/", ["a"])
        third = requires_provides_directory_class("/a", ["/a/b", "/a/b/c"])
        # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
        #  `List[typing.Any]`.
        dg_ok = DependencyGraph([second, first, third], layer_target="t")
        self.assertEqual(
            _fs_root_phases(first, layer_target="t"),
            list(dg_ok.ordered_phases()),
        )

        # `dg_bad`: changes `second` to get a cycle
        dg_bad = DependencyGraph(
            # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
            #  `List[typing.Any]`.
            [
                requires_provides_directory_class("a/b", ["a"]),
                first,
                third,
            ],
            layer_target="t",
        )
        self.assertEqual(
            _fs_root_phases(first, layer_target="t"),
            list(dg_bad.ordered_phases()),
        )

        self.assertEqual(
            [{second}, {third}],
            list(dg_ok.gen_dependency_order_items(self.provides_root)),
        )
        with self.assertRaisesRegex(AssertionError, "^Cycle in "):
            list(dg_bad.gen_dependency_order_items(self.provides_root))

    def test_phase_order(self) -> None:
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
        # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
        #  `List[Union[FakeRemovePaths, EnsureDirsExistItem, FilesystemRootItem]]`.
        dg = DependencyGraph([second, first, *rest], layer_target="t")
        self.assertEqual(
            _fs_root_phases(first, layer_target="t")
            + [(FakeRemovePaths.get_phase_builder, (second,))],
            list(dg.ordered_phases()),
        )
        self.assertEqual(
            rest,
            list(
                itertools.chain(
                    *dg.gen_dependency_order_items(self.provides_root)
                )
            ),
        )

    def test_parallel_items(self) -> None:
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def requires_provides_directory_class(requires_dir, provides_dirs):
            return TestImageItem(
                # pyre-fixme[6]: For 1st param expected `Iterator[ItemReq]` but got
                #  `List[RequireDirectory]`.
                reqs=[RequireDirectory(path=Path(requires_dir))],
                # pyre-fixme[6]: For 2nd param expected `Iterator[ItemProv]` but got
                #  `List[ProvidesDirectory]`.
                provs=[ProvidesDirectory(path=Path(d)) for d in provides_dirs],
            )

        a = requires_provides_directory_class("/", ["a"])
        a_children = {
            requires_provides_directory_class("/a", [f"/a/{x}"])
            for x in ("b", "c", "d")
        }
        # pyre-fixme[6]: For 1st param expected `Iterator[ImageItem]` but got
        #  `List[typing.Any]`.
        dg = DependencyGraph([a, *a_children], layer_target="t")
        self.assertEqual(
            [{a}, a_children],
            list(dg.gen_dependency_order_items(self.provides_root)),
        )


if __name__ == "__main__":
    unittest.main()
