#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
When writing tests, it is counterproductive to manually add traversal IDs to
subvolume inodes.  Instead, we add them automatically, using `InodeRepr` as
needed to flag the fact that two occurrences of an inode are the same inode
instance (i.e. hardlinks).  Refer to `test_subvolume.py` for usage examples.
"""
from typing import NamedTuple

from ..rendered_tree import map_bottom_up, RenderedTree, TraversalIDMaker


class InodeRepr(NamedTuple):
    """
    Use this instead of a plain string to represent an inode that occurs
    more than once in the filesystem (i.e. hardlinks).
    """

    ino_repr: str


def expected_subvol_add_traversal_ids(ser: RenderedTree):
    id_maker = TraversalIDMaker()
    return map_bottom_up(
        ser,
        lambda ino_repr: (
            id_maker.next_with_nonce(ino_repr).wrap(ino_repr.ino_repr)
            if isinstance(ino_repr, InodeRepr)
            else id_maker.next_unique().wrap(ino_repr)
        ),
    )
