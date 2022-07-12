#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Analyze and simplify IncompletInodes so that we can more easily check that
the salient parts of the filesystem are as we expect.

Similar to `get_frequency_of_selinux_xattrs` and `ItemFilters` from
`send_stream.py`, but for already-constructed filesystems.
"""

from typing import Tuple, Union

from antlir.btrfs_diff.incomplete_inode import IncompleteDir, IncompleteInode
from antlir.btrfs_diff.inode import Inode, InodeOwner


_SELINUX_XATTR = b"security.selinux"


def erase_mode_and_owner(
    ino: Union[IncompleteInode, Inode],
    *,
    owner: "InodeOwner",
    file_mode: int,
    dir_mode: int,
) -> None:
    if ino.owner == owner:
        # pyre-fixme[41]: Cannot reassign final attribute `owner`.
        ino.owner = None
    if (
        (ino.mode == dir_mode)
        if isinstance(ino, IncompleteDir)
        else (ino.mode == file_mode)
    ):
        # pyre-fixme[41]: Cannot reassign final attribute `mode`.
        ino.mode = None


def erase_utimes_in_range(
    ino: Union[IncompleteInode, Inode],
    start: Tuple[int, int],
    end: Tuple[int, int],
) -> None:
    utimes = ino.utimes
    if utimes is not None and all(
        start <= t <= end for t in (utimes.ctime, utimes.mtime, utimes.atime)
    ):
        # pyre-fixme[41]: Cannot reassign final attribute `utimes`.
        ino.utimes = None


def erase_selinux_xattr(ino: Union[IncompleteInode, Inode]) -> None:
    # Getting coverage for this line would force us to have a hard
    # dependency on running this test on an SELinux-enabled filesystem.
    # Mocking that seems like useless effort, so let's waive coverage.
    ino.xattrs.pop(_SELINUX_XATTR, None)  # pragma: no cover
