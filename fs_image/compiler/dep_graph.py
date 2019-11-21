#!/usr/bin/env python3
'''
To start, read the docblock of `provides.py`. The code in this file verifies
that a set of Items can be correctly installed (all requirements are
satisfied, etc).  It then computes an installation order such that every
Item is installed only after all of the Items that match its Requires have
already been installed.  This is known as dependency order or topological
sort.
'''
from collections import namedtuple
from typing import Iterator

from fs_image.compiler.items.common import ImageItem, PhaseOrder
from fs_image.compiler.items.make_subvol import FilesystemRootItem
from fs_image.compiler.items.phases_provide import PhasesProvideItem


# To build the item-to-item dependency graph, we need to first build up a
# complete mapping of {path, {items, requiring, it}}.  To validate that
# every requirement is satisfied, it is similarly useful to have access to a
# mapping of {path, {what, it, provides}}.  Lastly, we have to
# simultaneously examine a single item's requires() and provides() for the
# purposes of sanity checks.
#
# To avoid re-evaluating ImageItem.{provides,requires}(), we'll just store
# everything in these data structures:

ItemProv = namedtuple('ItemProv', ['provides', 'item'])
# NB: since the item is part of the tuple, we'll store identical
# requirements that come from multiple items multiple times.  This is OK.
ItemReq = namedtuple('ItemReq', ['requires', 'item'])
ItemReqsProvs = namedtuple('ItemReqsProvs', ['item_provs', 'item_reqs'])


class ValidatedReqsProvs:
    '''
    Given a set of Items (see the docblocks of `item.py` and `provides.py`),
    computes {'path': {ItemReqProv{}, ...}} so that we can build the
    DependencyGraph for these Items.  In the process validates that:
     - No one item provides or requires the same path twice,
     - Each path is provided by at most one item (could be relaxed later),
     - Every Requires is matched by a Provides at that path.
    '''
    def __init__(self, items):
        self.path_to_reqs_provs = {}

        for item in items:
            path_to_req_or_prov = {}  # Checks req/prov are sane within an item
            for req in item.requires():
                self._add_to_map(
                    path_to_req_or_prov, req, item,
                    add_to_map_fn=self._add_to_req_map,
                )
            for prov in item.provides():
                self._add_to_map(
                    path_to_req_or_prov, prov, item,
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
                        'At {}: nothing in {} matches the requirement {}'
                        .format(path, reqs_provs.item_provs, item_req)
                    )

    @staticmethod
    def _add_to_req_map(reqs_provs, req, item):
        reqs_provs.item_reqs.add(ItemReq(requires=req, item=item))

    @staticmethod
    def _add_to_prov_map(reqs_provs, prov, item):
        # I see no reason to allow provides-provides collisions.
        if len(reqs_provs.item_provs):
            raise RuntimeError(
                f'Both {reqs_provs.item_provs} and {prov} from {item} provide '
                'the same path'
            )
        reqs_provs.item_provs.add(ItemProv(provides=prov, item=item))

    def _add_to_map(
        self, path_to_req_or_prov, req_or_prov, item, add_to_map_fn
    ):
        # One ImageItem should not emit provides / requires clauses that
        # collide on the path.  Such duplication can always be avoided by
        # the item not emitting the "requires" clause that it knows it
        # provides.  Failing to enforce this invariant would make it easy to
        # bloat dependency graphs unnecessarily.
        other = path_to_req_or_prov.get(req_or_prov.path)
        assert other is None, 'Same path in {}, {}'.format(req_or_prov, other)
        path_to_req_or_prov[req_or_prov.path] = req_or_prov

        add_to_map_fn(
            self.path_to_reqs_provs.setdefault(
                req_or_prov.path,
                ItemReqsProvs(item_provs=set(), item_reqs=set()),
            ),
            req_or_prov,
            item
        )


class DependencyGraph:
    '''
    Given an iterable of ImageItems, validates their requires / provides
    structures, and populates indexes describing dependencies between items.
    The indexes make it easy to topologically sort the items.
    '''

    # Consumes a mix of dependency-ordered and `PhaseOrder`ed `ImageItem`s.
    def __init__(
        self, iter_items: {'Iterator of ImageItems'}, layer_target: str,
    ):
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
                    item.phase_order(), [],
                ).append(item)
        # If there is no MAKE_SUBVOL item, create an empty subvolume.
        make_subvol_items = self.order_to_phase_items.setdefault(
            PhaseOrder.MAKE_SUBVOL,
            [FilesystemRootItem(from_target=layer_target)],
        )
        assert len(make_subvol_items) == 1, make_subvol_items

    # Like ImageItems, the generated phases have a build(s: Subvol) operation.
    def ordered_phases(self):
        for _, items in sorted(
            self.order_to_phase_items.items(),
            key=lambda kv: kv[0].value,
        ):
            # We assume that all items in one phase share a builder factory
            all_builder_makers = {i.get_phase_builder for i in items}
            assert len(all_builder_makers) == 1, all_builder_makers
            yield all_builder_makers.pop(), tuple(items)

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
        ns.item_to_predecessors = {}  # {item: {items, it, requires}}
        ns.predecessor_to_items = {}  # {item: {items, requiring, it}}

        # For each path, treat items that provide something at that path as
        # predecessors of items that require something at the path.
        for _path, rp in ValidatedReqsProvs(
            self.items
        ).path_to_reqs_provs.items():
            for item_prov in rp.item_provs:
                requiring_items = ns.predecessor_to_items.setdefault(
                    item_prov.item, set()
                )
                for item_req in rp.item_reqs:
                    requiring_items.add(item_req.item)
                    ns.item_to_predecessors.setdefault(
                        item_req.item, set()
                    ).add(item_prov.item)

        ns.items_without_predecessors = \
            self.items - ns.item_to_predecessors.keys()

        return ns

    def gen_dependency_order_items(
        self, phases_provide: PhasesProvideItem,
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
                assert yield_idx == 0, f'{item}: PhasesProvideItem must be 1st'
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
        assert not ns.predecessor_to_items, \
            'Cycle in {}'.format(ns.predecessor_to_items)
