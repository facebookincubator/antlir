#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
If your test has a subvolume or a sendstream, these helpers here make it
easy to make assertions against its content. Grep around for usage examples.
"""
import copy
from io import BytesIO
from typing import Tuple

from ..freeze import freeze as btrfs_diff_freeze
from ..inode import InodeOwner
from ..inode_utils import (
    SELinuxXAttrStats,
    erase_mode_and_owner,
    erase_selinux_xattr,
    erase_utimes_in_range,
)
from ..parse_send_stream import parse_send_stream
from ..rendered_tree import RenderedTree, emit_non_unique_traversal_ids
from ..subvolume import Subvolume
from ..subvolume_set import SubvolumeSet, SubvolumeSetMutator
from .subvolume_utils import expected_subvol_add_traversal_ids


def pop_path(render, path):
    assert not isinstance(path, bytes), path  # Renderings are `str`
    parts = path.lstrip("/").split("/")
    for part in parts[:-1]:
        render = render[1][part]
    return render[1].pop(parts[-1])


# Future: this isn't really the right place for it, but for now we just have
# 2 places that need it, and it's annoying to create a whole new module just
# for this helper.
def check_common_rpm_render(test, rendered_subvol, yum_dnf: str):
    r = copy.deepcopy(rendered_subvol)

    # Ignore a bunch of yum / dnf / rpm spam

    if yum_dnf == "yum":
        (ino,) = pop_path(r, "var/log/yum.log")
        test.assertRegex(ino, r"^\(File m600 d[0-9]+\)$")
        for ignore_dir in ["var/cache/yum", "var/lib/yum"]:
            ino, _ = pop_path(r, ignore_dir)
            test.assertEqual("(Dir)", ino)
    elif yum_dnf == "dnf":
        test.assertEqual(
            ["(Dir)", {"dnf": ["(Dir)", {"modules.d": ["(Dir)", {}]}]}],
            pop_path(r, "etc"),
        )
        for logname in [
            "dnf.log",
            "dnf.librepo.log",
            "dnf.rpm.log",
            "hawkey.log",
        ]:
            (ino,) = pop_path(r, f"var/log/{logname}")
            test.assertRegex(ino, r"^\(File d[0-9]+\)$", logname)
        for ignore_dir in ["var/cache/dnf", "var/lib/dnf"]:
            ino, _ = pop_path(r, ignore_dir)
            test.assertEqual("(Dir)", ino)
    else:
        raise AssertionError(yum_dnf)

    ino, _ = pop_path(r, "var/lib/rpm")
    test.assertEqual("(Dir)", ino)

    test.assertEqual(
        [
            "(Dir)",
            {
                "dev": ["(Dir)", {}],
                "meta": [
                    "(Dir)",
                    {
                        "private": [
                            "(Dir)",
                            {
                                "opts": [
                                    "(Dir)",
                                    {
                                        "artifacts_may_require_repo": [
                                            "(File d2)"
                                        ]
                                    },
                                ]
                            },
                        ]
                    },
                ],
                "var": [
                    "(Dir)",
                    {
                        "cache": ["(Dir)", {}],
                        "lib": ["(Dir)", {}],
                        "log": ["(Dir)", {}],
                    },
                ],
            },
        ],
        r,
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
    build_end_time: Tuple[int, int] = (2 ** 64 - 1, 2 ** 32 - 1),
):
    # Check that our sendstreams completely specified the subvolumes.
    for ino in btrfs_diff_freeze(subvols).inodes():
        ino.assert_valid_and_complete()

    # Render the demo subvolumes after stripping all the predictable
    # metadata to make our "expected" view of the filesystem shorter.
    selinux_stats = SELinuxXAttrStats(subvols.inodes())
    for ino in subvols.inodes():
        erase_mode_and_owner(
            ino, owner=InodeOwner(uid=0, gid=0), file_mode=0o644, dir_mode=0o755
        )
        erase_utimes_in_range(ino, start=build_start_time, end=build_end_time)
        erase_selinux_xattr(ino, selinux_stats.most_common())


# Often, we just want to render 1 sendstream
def render_sendstream(sendstream: bytes) -> "RenderedTree":
    subvol_set = SubvolumeSet.new()
    subvolume = add_sendstream_to_subvol_set(subvol_set, sendstream)
    prepare_subvol_set_for_render(subvol_set)
    return render_subvolume(subvolume)
