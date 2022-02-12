#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is used by RepoSizer, which is part of our OSS for buck images. We've
duplicated it here given that it's a simple library unlikely to be updated, and
because linking it from libfb to our OSS release adds some complexity that's
easily avoided by just placing the file in this directory.
"""


class UnionFind(object):
    """Special implementation of UnionFind (aka disjoint set) where nodes are
    stored as key value pairs in a dictionary and not in an actual tree
    with linked nodes. Upon a union call, the intermediate parents of the
    node will be replaced with the representative (aka absolute grandparent)
    therefore the full hierarchy will be lost.

    This class is not suitable for situations when original connections of
    the nodes need to be preserved.

    for more information about Union Find check:
    https://en.wikipedia.org/wiki/Disjoint-set_data_structure
    """

    def __init__(self) -> None:
        """Constructor.
        Note: This implementation of Union Find does not need to know
        the number of nodes in advance.
        """
        # This variable holds a map from any node to its ultimate parent
        # (representative)
        self._representative_dict = {}

    def union(self, id1, id2) -> None:
        """Join two nodes <id1> and <id2> by connecting the representatives.
        The function adds nodes to the data structure if they do not already
        exist (indirectly through find() function). If at least one of the
        nodes exists, it performs partial path compression indirectly if
        possible (again through find())
        """
        representative1 = self.find(id1)
        representative2 = self.find(id2)
        self._representative_dict[representative1] = representative2

    def find(self, id):
        """Find and return the representative node for <id>.
        The path from <id> to the representative will be compressed by
        connecting <id> directly to the representative.
        If <id> does not exist, it is added to the data structure as a new
        representative of itself.
        """
        if id not in self._representative_dict:
            self.add(id)
            return id

        parent = self._representative_dict[id]
        if parent == id:
            return id

        root = self.find(parent)
        self._representative_dict[
            id
        ] = root  # imperative, path compression heuristic !
        return root

    def flatten(self) -> None:
        """Flattens the tree by running find with path compression on all ids"""
        for key in self._representative_dict:
            self.find(key)

    def add(self, id) -> None:
        """Add a new node as a representative of itself"""
        if id not in self._representative_dict:
            self._representative_dict[id] = id

    def items(self):
        """Return a generator for <id>, <representative> pairs"""
        for key in self._representative_dict:
            yield key, self.find(key)

    # -----------------------------
    # End of public interface

    def _parent(self, id):
        """Returns the current parent for <id>. Unlike find it does not find
        the representative (aka root grandparent).
        """
        return self._representative_dict[id]

    def __iter__(self):
        """Enables iterations over ids"""
        for id in self._representative_dict:
            yield id
