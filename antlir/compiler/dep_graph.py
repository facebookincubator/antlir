#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
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
from typing import Iterator, Union, NamedTuple, Dict, Callable, Set

from antlir.compiler.items.common import ImageItem, PhaseOrder
from antlir.compiler.items.ensure_dir_exists import EnsureDirExistsItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.phases_provide import PhasesProvideItem

from .requires_provides import (
    ProvidesPathObject,
    ProvidesDirectory,
    PathRequiresPredicate,
)


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
    provides: ProvidesPathObject
    item: ImageItem


# NB: since the item is part of the tuple, we'll store identical
# requirements that come from multiple items multiple times.  This is OK.
class ItemReq(NamedTuple):
    requires: PathRequiresPredicate
    item: ImageItem


class ItemReqsProvs(NamedTuple):
    item_provs: Set[ItemProv]
    item_reqs: Set[ItemReq]


ReqOrProv = Union[ProvidesPathObject, PathRequiresPredicate]
# See the comments in _add_to_prov_map
_ALLOWED_COLLISIONS = frozenset({EnsureDirExistsItem})


class ValidatedReqsProvs:
    """
    Given a set of Items (see the docblocks of `item.py` and `provides.py`),
    computes {'path': {ItemReqProv{}, ...}} so that we can build the
    DependencyGraph for these Items.  In the process validates that:
     - No one item provides or requires the same path twice,
     - Each path is provided by at most one item (excl. _ALLOWED_COLLISIONS),
     - Every Requires is matched by a Provides at that path.
    """

    def __init__(self, items: Set[ImageItem]):
        self.path_to_reqs_provs = {}

        for item in items:
            path_to_req_or_prov = {}  # Checks req/prov are sane within an item
            for req in item.requires():
                self._add_to_map(
                    path_to_req_or_prov,
                    req,
                    item,
                    add_to_map_fn=self._add_to_req_map,
                )
            for prov in item.provides():
                self._add_to_map(
                    path_to_req_or_prov,
                    prov,
                    item,
                    add_to_map_fn=self._add_to_prov_map,
                )

        # Validate that all requirements are satisfied.
        for path, reqs_provs in self.path_to_reqs_provs.items():
            for item_req in reqs_provs.item_reqs:
                for item_prov in reqs_provs.item_provs:
                    if item_prov.provides.matches(
                        self.path_to_reqs_provs, item_req.requires
                    ):
                        break
                else:
                    raise RuntimeError(
                        f"At {path}: nothing in {reqs_provs.item_provs} "
                        f"matches the requirement {item_req}"
                    )

    @staticmethod
    def _add_to_req_map(
        reqs_provs: ItemReqsProvs, req: PathRequiresPredicate, item: ImageItem
    ):
        reqs_provs.item_reqs.add(ItemReq(requires=req, item=item))

    @staticmethod
    def _add_to_prov_map(
        reqs_provs: ItemReqsProvs, prov: ProvidesPathObject, item: ImageItem
    ):
        """For the majority of cases, we do not allow two `provides` to collide
        on the same path.

        The sole case where this is supported is when there are any number of
        EnsureDirExists items, and at most one other directory provider of a
        type other than EnsureDirExists. This is done because EnsureDirExists
        are explicitly run last for a given path (see comments in
        _add_dir_deps_for_item_provs), and check corresponding attributes on the
        path they're about to create. As such, any number of them may exist for
        a given path. We allow one other non-EnsureDirExists directory provider
        as its attributes will also be checked. More than one is disallowed as
        it could result in non-determinism, as we could only support that if we
        were certain an EnsureDirExists also existed for the path, which the
        data model is not currently set up to support.
        """
        new_item_prov = ItemProv(provides=prov, item=item)
        if reqs_provs.item_provs:
            collision_provs = [
                ip.provides
                for ip in [*reqs_provs.item_provs, new_item_prov]
                if type(ip.item) not in _ALLOWED_COLLISIONS
            ]
            if collision_provs and (
                len(collision_provs) > 1
                or not isinstance(collision_provs[0], ProvidesDirectory)
            ):
                raise RuntimeError(
                    f"Both {reqs_provs.item_provs} and {prov} from {item} "
                    "provide the same path"
                )
        reqs_provs.item_provs.add(new_item_prov)

    def _add_to_map(
        self,
        path_to_req_or_prov: Dict[str, ReqOrProv],
        req_or_prov: ReqOrProv,
        item: ImageItem,
        add_to_map_fn: Callable[[ItemReqsProvs, ReqOrProv, ImageItem], None],
    ):
        # One ImageItem should not emit provides / requires clauses that
        # collide on the path.  Such duplication can always be avoided by
        # the item not emitting the "requires" clause that it knows it
        # provides.  Failing to enforce this invariant would make it easy to
        # bloat dependency graphs unnecessarily.
        other = path_to_req_or_prov.get(req_or_prov.path)
        assert other is None, "Same path in {}, {}".format(req_or_prov, other)
        path_to_req_or_prov[req_or_prov.path] = req_or_prov

        add_to_map_fn(
            self.path_to_reqs_provs.setdefault(
                req_or_prov.path,
                ItemReqsProvs(item_provs=set(), item_reqs=set()),
            ),
            req_or_prov,
            item,
        )


class DependencyGraph:
    """
    Given an iterable of ImageItems, validates their requires / provides
    structures, and populates indexes describing dependencies between items.
    The indexes make it easy to topologically sort the items.
    """

    # Consumes a mix of dependency-ordered and `PhaseOrder`ed `ImageItem`s.
    def __init__(self, iter_items: Iterator[ImageItem], layer_target: str):
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
        # If we have a foreign layer, it must be the only item, besides the
        # mandatory `MAKE_SUBVOL` added above.
        foreign = self.order_to_phase_items.get(PhaseOrder.FOREIGN_LAYER)
        if foreign:
            assert len(foreign) == 1, foreign
            assert not self.items, self.items
            assert set(self.order_to_phase_items.keys()) == {
                PhaseOrder.FOREIGN_LAYER,
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
    def _add_dir_deps_for_item_provs(ns, item_provs: Set[ItemProv]):
        """EnsureDirExists items are a special case in the dependency graph in
        that, for a given path, we want to ensure they're the last providers to
        be run. This is because they're the only items that will explicitly
        check the attributes of the given path to ensure they match the provided
        stat args. Thus, if another directory provider were to run before them,
        it's possible it would unexpectedly modify the attributes of the
        directory provided by the EnsureDirExists item.

        To enforce this, we explicitly add dependency edges from all
        non-EnsureDirExists items to all EnsureDirExists items.
        """
        ede_item_provs = {
            x for x in item_provs if isinstance(x.item, EnsureDirExistsItem)
        }
        non_ede_item_provs = item_provs - ede_item_provs
        # Guaranteed by checks in _add_to_prov_map
        assert len(non_ede_item_provs) <= 1
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
        ns.item_to_predecessors = defaultdict(set)
        # {item: {items, requiring, it}}
        ns.predecessor_to_items = defaultdict(set)

        # For each path, treat items that provide something at that path as
        # predecessors of items that require something at the path.
        for _path, rp in ValidatedReqsProvs(
            self.items
        ).path_to_reqs_provs.items():
            self._add_dir_deps_for_item_provs(ns, rp.item_provs)
            for item_prov in rp.item_provs:
                for item_req in rp.item_reqs:
                    ns.predecessor_to_items[item_prov.item].add(item_req.item)
                    ns.item_to_predecessors[item_req.item].add(item_prov.item)

        ns.items_without_predecessors = (
            self.items - ns.item_to_predecessors.keys()
        )

        return ns

    def gen_dependency_order_items(
        self, phases_provide: PhasesProvideItem
    ) -> Iterator[ImageItem]:
        ns = self._prep_item_predecessors(phases_provide)
        yield_idx = 0
        while ns.items_without_predecessors:
            # "Install" an item that has no unsatisfied dependencies.
            item = ns.items_without_predecessors.pop()
            # `_prep_item_predecessors` ensures that we will encounter
            # `phases_provide` whose `provides` describes the state of the
            # layer after the phases had run (before we build items).
            if item is phases_provide:
                # This item deliberately lacks `build()`, so don't yield it.
                assert yield_idx == 0, f"{item}: PhasesProvideItem must be 1st"
            else:
                yield item
            yield_idx += 1

            # All items, which had `item` was a dependency, must have their
            # "predecessors" sets updated
            for requiring_item in ns.predecessor_to_items[item]:
                predecessors = ns.item_to_predecessors[requiring_item]
                predecessors.remove(item)
                if not predecessors:
                    ns.items_without_predecessors.add(requiring_item)
                    # With no more predecessors, this will no longer be used.
                    del ns.item_to_predecessors[requiring_item]

            # We won't need this value again, and this lets us detect cycles.
            del ns.predecessor_to_items[item]

        # Initially, every item was indexed here. If there's anything left,
        # we must have a cycle. Future: print a cycle to simplify debugging.
        assert not ns.predecessor_to_items, "Cycle in {}".format(
            ns.predecessor_to_items
        )
