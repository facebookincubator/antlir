#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
If your test has a subvolume or a sendstream, these helpers here make it
easy to make assertions against its content. Grep around for usage examples.
"""
from io import BytesIO
from typing import Tuple

from antlir.btrfs_diff.freeze import freeze as btrfs_diff_freeze
from antlir.btrfs_diff.inode import InodeOwner
from antlir.btrfs_diff.inode_utils import (
    erase_mode_and_owner,
    erase_selinux_xattr,
    erase_utimes_in_range,
)
from antlir.btrfs_diff.parse_send_stream import parse_send_stream
from antlir.btrfs_diff.rendered_tree import (
    emit_non_unique_traversal_ids,
    RenderedTree,
)
from antlir.btrfs_diff.subvolume import Subvolume
from antlir.btrfs_diff.subvolume_set import SubvolumeSet, SubvolumeSetMutator
from antlir.btrfs_diff.tests.subvolume_utils import (
    expected_subvol_add_traversal_ids,
)


def expected_rendering(expected_subvol):
    "Takes a `RenderedTree` with `InodeRepr` for some of the inodes."
    return emit_non_unique_traversal_ids(
        expected_subvol_add_traversal_ids(expected_subvol)
    )


def render_subvolume(subvol: "Subvolume") -> "RenderedTree":
    return emit_non_unique_traversal_ids(btrfs_diff_freeze(subvol).render())


def add_sendstream_to_subvol_set(subvols: SubvolumeSet, sendstream: bytes):
    parsed = parse_send_stream(BytesIO(sendstream))
    mutator = SubvolumeSetMutator.new(subvols, next(parsed))
    for i in parsed:
        mutator.apply_item(i)
    return mutator.subvolume


# We could do this on each `mutator.subvol` in `add_...`, but that would
# make `add_...` less reusable.  E.g., it would preclude cross-subvolume
# clone detection.
def prepare_subvol_set_for_render(
    subvols: SubvolumeSet,
    build_start_time: Tuple[int, int] = (0, 0),
    build_end_time: Tuple[int, int] = (2**64 - 1, 2**32 - 1),
):
    # Check that our sendstreams completely specified the subvolumes.
    for ino in btrfs_diff_freeze(subvols).inodes():
        ino.assert_valid_and_complete()

    # Render the demo subvolumes after stripping all the predictable
    # metadata to make our "expected" view of the filesystem shorter.
    for ino in subvols.inodes():
        erase_mode_and_owner(
            ino, owner=InodeOwner(uid=0, gid=0), file_mode=0o644, dir_mode=0o755
        )
        erase_utimes_in_range(ino, start=build_start_time, end=build_end_time)
        erase_selinux_xattr(ino)


# Often, we just want to render 1 sendstream
def render_sendstream(sendstream: bytes) -> "RenderedTree":
    subvol_set = SubvolumeSet.new()
    subvolume = add_sendstream_to_subvol_set(subvol_set, sendstream)
    prepare_subvol_set_for_render(subvol_set)
    return render_subvolume(subvolume)
