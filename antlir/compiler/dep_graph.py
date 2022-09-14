#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
To start, read the docblock of `provides.py`. The code in this file verifies
that a set of Items can be correctly installed (all requirements are
satisfied, etc).  It then computes an installation order such that every
Item is installed only after all of the Items that match its Requires have
already been installed.  This is known as dependency order or topological
sort.
"""
from collections import defaultdict
from typing import (
    Dict,
    Generator,
    Iterable,
    Iterator,
    List,
    NamedTuple,
    Optional,
    Set,
)

from antlir.compiler.items.common import ImageItem, PhaseOrder
from antlir.compiler.items.ensure_dirs_exist import EnsureDirsExistItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem

from antlir.compiler.requires_provides import (
    Provider,
    ProvidesDirectory,
    ProvidesPath,
    ProvidesSymlink,
    Requirement,
    RequirePath,
    RequireSymlink,
)
from antlir.errors import UserError
from antlir.fs_utils import Path


# To build the item-to-item dependency graph, we need to first build up a
# complete mapping of {path, {items, requiring, it}}.  To validate that
# every requirement is satisfied, it is similarly useful to have access to a
# mapping of {path, {what, it, provides}}.  Lastly, we have to
# simultaneously examine a single item's requires() and provides() for the
# purposes of sanity checks.
#
# To avoid re-evaluating ImageItem.{provides,requires}(), we'll just store
# everything in these data structures:


class ItemProv(NamedTuple):
    provides: Provider
    item: ImageItem

    def conflicts(self, other: "ItemProv") -> bool:
        """
        Checks for conflicts between provided resource and item providing
        the resource.

        NB: This functionality is not on ImageItem and its subclasses because
        conflicts must be symmetric by type(item) and it's easier to see/test
        the rules in one place.

        For the majority of cases, we do not allow two `provides` to collide
        on the same path. The following cases are allowed:

        1. There can be any number of EnsureDirsExist items, and at most one
        other directory provider of a type other than EnsureDirsExist. This is
        done because EnsureDirsExist are explicitly run last for a given path
        (see comments in _add_dir_deps_for_item_provs), and check corresponding
        attributes on the path they're about to create. As such, any number of
        them may exist for a given path. We allow one other non-EnsureDirsExist
        directory provider as its attributes will also be checked. More than one
        is disallowed as it could result in non-determinism, as we could only
        support that if we were certain an EnsureDirsExist also existed for the
        path, which the data model is not currently set up to support.

        2. Symlink dir and file items may duplicate, as long as they are the
        same type (all dirs or all files) and have the same source.
        """
        for a, b in ((self, other), (other, self)):
            if isinstance(a.item, EnsureDirsExistItem):
                assert isinstance(
                    a.provides, ProvidesDirectory
                ), "EnsureDirsExistItem must provide a directory"
                return not isinstance(
                    b.provides, (ProvidesDirectory, ProvidesSymlink)
                )

        for it in (SymlinkToDirItem, SymlinkToFileItem):
            if isinstance(self.item, it) and isinstance(other.item, it):
                return (
                    # pyre-fixme[16]: `ImageItem` has no attribute `dest`.
                    self.item.dest != other.item.dest
                    # pyre-fixme[16]: `ImageItem` has no attribute `source`.
                    or self.item.source != other.item.source
                )

        return True


# NB: since the item is part of the tuple, we'll store identical
# requirements that come from multiple items multiple times.  This is OK.
class ItemReq(NamedTuple):
    requires: Requirement
    item: ImageItem


class ItemReqsProvs(NamedTuple):
    item_provs: Set[ItemProv]
    item_reqs: Set[ItemReq]

    def _item_self_conflict(self, item: ImageItem) -> bool:
        """
        One ImageItem should not emit provides or requires clauses that
        collide on the path. Such duplication can always be avoided by
        the item 1. not emitting the requires clause that it knows it
        provides, and 2. not emitting multiple requires or provides clauses
        that map to the same `ItemReqsProvs`. Failing to enforce this
        invariant would make it easy to bloat dependency graphs unnecessarily.

        NB: Two equivalent (i.e. where __eq__ returns True) `ImageItem`s may
        emit colliding provides or requires. This check is only to ensure that
        the exact same `ImageItem` does not conflict with itself.
        """
        return any(ip.item is item for ip in self.item_provs) or any(
            ir.item is item for ir in self.item_reqs
        )

    def add_item_req(self, req: Requirement, item: ImageItem) -> None:
        if self._item_self_conflict(item):
            raise UserError(f"{req} from {item} conflicts in {self}")
        self.item_reqs.add(ItemReq(requires=req, item=item))

    def add_item_prov(self, prov: Provider, item: ImageItem) -> None:
        if self._item_self_conflict(item):
            raise UserError(f"{prov} from {item} conflicts in {self}")

        # For the majority of cases, we do not allow two `Providers` to
        # provide the same `Requirement`. There are some cases that are allowed
        # (e.g. duplicate EnsureDirsExist). See `ItemProv.conflicts` for full
        # rules implementation.
        new_ip = ItemProv(provides=prov, item=item)
        for ip in self.item_provs:
            if new_ip.conflicts(ip):
                raise UserError(f"{new_ip} conflicts with {ip}")
        self.item_provs.add(new_ip)

    def item_req_fulfilled(self, item_req: ItemReq) -> bool:
        return any(
            item_prov.provides.provides(item_req.requires)
            for item_prov in self.item_provs
        )

    def unfulfilled_item_reqs(self) -> List[ItemReq]:
        return [ir for ir in self.item_reqs if not self.item_req_fulfilled(ir)]

    def symlink_item_prov(self) -> Optional[ItemProv]:
        for ip in self.item_provs:
            if isinstance(ip.provides, ProvidesSymlink):
                return ip
        return None


def _symlink_target_normpath(symlink: Path, target: Path) -> Path:
    """
    Returns a normpath of a symlink's target based on the symlink's dir.

    In most cases, `Path.realpath` can and should be used when resolving
    symlinks. `realpath` will follow all symlinks until a final, concrete
    path is found which is desired in a large majority of cases. However, it
    requires that the symlinks/files actually exist.

    This function is for doing a single symlink resolve when paths have not
    been committed to a filesystem.

    Symlink targets can be absolute paths (starts with `/`) or relative to
    the containing directory.
    """
    return (symlink.dirname() / target).normpath()


class PathItemReqsProvs:
    """
    PathItemReqsProvs validates RequirePath and ProvidesPath from ImageItems.

    Logic for path-specific Requirements and Providers is split out here since:

    1. a single path can only satisfy one type of Requirement (e.g. directory,
      file, or symlink).
    2. multiple Providers of the same path may be supported (see
      ItemProv.conflicts).
    3. when one or more symlinked directories are on a required path, a
      recursive search must be performed to validate whether the requirement
      is fulfilled. See _realpath_item_provs
    """

    path_to_item_reqs_provs: Dict[Path, ItemReqsProvs]

    def __init__(self) -> None:
        self.path_to_item_reqs_provs = {}

    def _get_item_reqs_provs(self, path: Path) -> ItemReqsProvs:
        return self.path_to_item_reqs_provs.setdefault(
            path,
            ItemReqsProvs(item_provs=set(), item_reqs=set()),
        )

    def add_requirement(self, req: RequirePath, item: ImageItem) -> None:
        self._get_item_reqs_provs(req.path).add_item_req(req, item)

    def add_provider(self, prov: ProvidesPath, item: ImageItem) -> None:
        self._get_item_reqs_provs(prov.req.path).add_item_prov(prov, item)

    def validate(self) -> None:
        for path, irps in self.path_to_item_reqs_provs.items():
            irs = irps.unfulfilled_item_reqs()
            if not irs:
                continue

            for ir in irs:
                if isinstance(ir.requires, RequireSymlink):
                    raise UserError(
                        f"{path}: {irps.item_provs} does not provide {ir}; "
                        "RequireSymlink must be explicitly fulfilled"
                    )

            symlink_item_provs = self._realpath_item_provs(path)
            if symlink_item_provs:
                # make sure a symlink prov fulfills the requirement
                if all(
                    any(
                        isinstance(ir.requires, type(ip.provides.req))
                        for ip in symlink_item_provs
                    )
                    for ir in irs
                ):
                    # Add ItemProvs that provide the symlink path so that
                    # DependencyGraph knows those ImageItems are prequisites.
                    irps.item_provs.update(symlink_item_provs)
                    continue

            raise UserError(f"{path}: {irps.item_provs} does not provide {irs}")

    def item_reqs_provs(self) -> Generator[ItemReqsProvs, None, None]:
        yield from self.path_to_item_reqs_provs.values()

    def _realpath_item_provs(
        self,
        path: Path,
        # pyre-fixme[9]: history has type `Set[Path]`; used as `None`.
        history: Set[Path] = None,
    ) -> Optional[Set[ItemProv]]:
        """Recursively walk subsections of path to see if symlinks provide
        the full path. Returns ItemProvs that provide the symlinks and
        underlying target if (and only if) traversal succeeds.
        """
        if history is None:
            history = set()

        if path in history:
            raise RuntimeError(
                f"Circular realpath, revisiting {path} in {history}"
            )
        else:
            history.add(path)

        assert path.startswith(b"/"), f"{path} must be absolute"
        path_parts = path.split(b"/")[1:]

        search_path = Path("/")
        while path_parts:
            path_part = path_parts[0]
            path_parts = path_parts[1:]
            search_path = search_path / path_part

            irps = self.path_to_item_reqs_provs.get(search_path)
            if not irps:
                return None

            symlink_item_prov = irps.symlink_item_prov()
            if not symlink_item_prov:
                continue

            # pyre-fixme[16]: `Requirement` has no attribute `target`.
            symlink_target = symlink_item_prov.provides.req.target
            search_path_realpath = _symlink_target_normpath(
                search_path, symlink_target
            )
            if path_parts:
                search_path_realpath /= Path.join(*path_parts)
            nested_provs = self._realpath_item_provs(
                search_path_realpath, history
            )
            if nested_provs is None:
                return None
            return {symlink_item_prov} | nested_provs
        return self.path_to_item_reqs_provs[search_path].item_provs


class ValidatedReqsProvs:
    """
    Given a set of Items (see the docblocks of `item.py` and `provides.py`),
    computes {key: {ItemReqProv{}, ...}} so that we can build the
    DependencyGraph for these Items.  In the process validates that:
     - No one ImageItems provides or requires the same path twice,
     - Each Requirement is Provided by one or more ImageItems without
       conflicts.
    """

    _item_reqs_provs: Dict[Requirement, ItemReqsProvs]
    # _path_item_reqs_provs maps path requires/provides and includes special
    # handling for collisions. See `PathItemReqsProvs` for more info.
    _path_item_reqs_provs: PathItemReqsProvs

    def __init__(self, items: Set[ImageItem]) -> None:
        self._item_reqs_provs = {}
        self._path_item_reqs_provs = PathItemReqsProvs()

        for item in items:
            # pyre-fixme[16]: `ImageItem` has no attribute `requires`.
            for req in item.requires():
                if isinstance(req, RequirePath):
                    self._path_item_reqs_provs.add_requirement(req, item)
                    continue
                self._get_item_reqs_provs(req).add_item_req(req, item)

            # pyre-fixme[16]: `ImageItem` has no attribute `provides`.
            for prov in item.provides():
                if isinstance(prov, ProvidesPath):
                    self._path_item_reqs_provs.add_provider(prov, item)
                    continue
                self._get_item_reqs_provs(prov.req).add_item_prov(prov, item)

        # Validate that all requirements are satisfied.
        self._path_item_reqs_provs.validate()
        for irps in self._item_reqs_provs.values():
            for item_req in irps.unfulfilled_item_reqs():
                raise UserError(
                    f"{irps.item_provs} does not provide {item_req}"
                )

    def _get_item_reqs_provs(self, req: Requirement) -> ItemReqsProvs:
        return self._item_reqs_provs.setdefault(
            req,
            ItemReqsProvs(item_provs=set(), item_reqs=set()),
        )

    def item_reqs_provs(self) -> Generator[ItemReqsProvs, None, None]:
        yield from self._item_reqs_provs.values()
        yield from self._path_item_reqs_provs.item_reqs_provs()


class DependencyGraph:
    """
    Given an iterable of ImageItems, validates their requires / provides
    structures, and populates indexes describing dependencies between items.
    The indexes make it easy to topologically sort the items.
    """

    # Consumes a mix of dependency-ordered and `PhaseOrder`ed `ImageItem`s.
    def __init__(
        self, iter_items: Iterable[ImageItem], layer_target: str
    ) -> None:
        # Without deduping, dependency diamonds would cause a lot of
        # redundant work below.  `_prep_item_predecessors` mutates this.
        self.items = set()
        # While deduplicating `ImageItem`s, let's also split out the phases.
        self.order_to_phase_items = {}
        for item in iter_items:
            if item.phase_order() is None:
                self.items.add(item)
            else:
                self.order_to_phase_items.setdefault(
                    item.phase_order(), []
                ).append(item)
        # If there is no MAKE_SUBVOL item, create an empty subvolume.
        make_subvol_items = self.order_to_phase_items.setdefault(
            PhaseOrder.MAKE_SUBVOL,
            [FilesystemRootItem(from_target=layer_target)],
        )
        assert len(make_subvol_items) == 1, make_subvol_items

        # If we have a genrule layer, it must be the only item, besides the
        # mandatory `MAKE_SUBVOL`.
        genrule = self.order_to_phase_items.get(PhaseOrder.GENRULE_LAYER)
        if genrule:
            assert len(genrule) == 1, genrule
            assert not self.items, self.items

            assert set(self.order_to_phase_items.keys()) == {
                PhaseOrder.GENRULE_LAYER,
                PhaseOrder.MAKE_SUBVOL,
            }, self.order_to_phase_items

    # Like ImageItems, the generated phases have a build(s: Subvol) operation.
    def ordered_phases(self):
        for _, items in sorted(
            self.order_to_phase_items.items(), key=lambda kv: kv[0].value
        ):
            # We assume that all items in one phase share a builder factory
            all_builder_makers = {i.get_phase_builder for i in items}
            assert len(all_builder_makers) == 1, all_builder_makers
            yield all_builder_makers.pop(), tuple(items)

    @staticmethod
    def _add_dir_deps_for_item_provs(ns, item_provs: Set[ItemProv]) -> None:
        """EnsureDirsExist items are a special case in the dependency graph in
        that, for a given path, we want to ensure they're the last providers to
        be run. This is because they're the only items that will explicitly
        check the attributes of the given path to ensure they match the provided
        stat args. Thus, if another directory provider were to run before them,
        it's possible it would unexpectedly modify the attributes of the
        directory provided by the EnsureDirsExist item.

        To enforce this, we explicitly add dependency edges from all
        non-EnsureDirsExist items to all EnsureDirsExist items.
        """
        ede_item_provs = {
            x for x in item_provs if isinstance(x.item, EnsureDirsExistItem)
        }
        non_ede_item_provs = item_provs - ede_item_provs

        # Guaranteed by checks in ItemReqsProvs.add_item_prov
        symlink_item_provs = {
            x for x in item_provs if isinstance(x.provides, ProvidesSymlink)
        }
        assert (
            len(non_ede_item_provs - symlink_item_provs) <= 1
        ), f"{item_provs}"

        for item_prov in non_ede_item_provs:
            for ede_item_prov in ede_item_provs:
                ns.item_to_predecessors[ede_item_prov.item].add(item_prov.item)
                ns.predecessor_to_items[item_prov.item].add(ede_item_prov.item)

    # Separated so that unit tests can check the internal state.
    def _prep_item_predecessors(self, phases_provide: PhasesProvideItem):
        # The `ImageItem` part of the build needs an item that `provides`
        # the filesystem as it exists after the phases get built.
        #
        # This item computes `provides()` for dependency resolution using
        # the modified subvolume.  This isn't too scary since the rest of
        # this function is guaranteed to evaluate this item's `provides()`
        # before any `ImageItem.build()`.
        self.items.add(phases_provide)

        class Namespace:
            pass

        # An item is only added here if it requires at least one other item,
        # otherwise it goes in `.items_without_predecessors`.
        ns = Namespace()
        # {item: {items, it, requires}}
        # pyre-fixme[16]: `Namespace` has no attribute `item_to_predecessors`.
        ns.item_to_predecessors = defaultdict(set)
        # {item: {items, requiring, it}}
        # pyre-fixme[16]: `Namespace` has no attribute `predecessor_to_items`.
        ns.predecessor_to_items = defaultdict(set)

        # For each path, treat items that provide something at that path as
        # predecessors of items that require something at the path.
        for rp in ValidatedReqsProvs(self.items).item_reqs_provs():
            self._add_dir_deps_for_item_provs(ns, rp.item_provs)
            for item_prov in rp.item_provs:
                for item_req in rp.item_reqs:
                    ns.predecessor_to_items[item_prov.item].add(item_req.item)
                    ns.item_to_predecessors[item_req.item].add(item_prov.item)

        # pyre-fixme[16]: `Namespace` has no attribute
        # `items_without_predecessors`.
        ns.items_without_predecessors = (
            self.items - ns.item_to_predecessors.keys()
        )

        return ns

    def gen_dependency_order_items(
        self, phases_provide: PhasesProvideItem
    ) -> Iterator[List[ImageItem]]:
        if not self.items:
            # Skip evaluating `PhasesProvideItem` if there's no work to be
            # done in the final phase.  This is useful because otherwise
            # `PhasesProvideItem` would `stat` the entire layer FS.
            return
        ns = self._prep_item_predecessors(phases_provide)
        yield_index = 0
        while ns.items_without_predecessors:
            items = ns.items_without_predecessors.copy()
            ns.items_without_predecessors.clear()
            if yield_index == 0:
                assert (
                    len(items) == 1
                ), f"PhasesProvideItem must be alone in the first step: {items}"
                item = next(iter(items))
                assert (
                    item is phases_provide
                ), f"{item}: PhasesProvideItem must be 1st"

            for item in items:
                # `_prep_item_predecessors` ensures that we will encounter
                # `phases_provide` whose `provides` describes the state of the
                # layer after the phases had run (before we build items).
                if yield_index != 0:
                    assert (
                        item is not phases_provide
                    ), f"{item}: PhasesProvideItem must be 1st"

                # All items, which had `item` was a dependency, must have their
                # "predecessors" sets updated
                for requiring_item in ns.predecessor_to_items[item]:
                    predecessors = ns.item_to_predecessors[requiring_item]
                    predecessors.remove(item)
                    if not predecessors:
                        ns.items_without_predecessors.add(requiring_item)
                        # With no more predecessors, this will no longer be
                        # used.
                        del ns.item_to_predecessors[requiring_item]

                # We won't need this value again, and this lets us detect
                # cycles.
                del ns.predecessor_to_items[item]

            # Don't yield PhasesProvideItem, it lacks `build()`
            if yield_index > 0:
                # All of these items have no predecessors, so by definition they
                # can run in parallel
                yield items
            yield_index += 1

        # Initially, every item was indexed here. If there's anything left,
        # we must have a cycle. Future: print a cycle to simplify debugging.
        assert not ns.predecessor_to_items, "Cycle in {}".format(
            ns.predecessor_to_items
        )
