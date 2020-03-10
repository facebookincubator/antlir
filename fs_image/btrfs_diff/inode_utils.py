#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Analyze and simplify IncompletInodes so that we can more easily check that
the salient parts of the filesystem are as we expect.

Similar to `get_frequency_of_selinux_xattrs` and `ItemFilters` from
`send_stream.py`, but for already-constructed filesystems.
'''

from collections import Counter
from typing import Optional, Tuple, Union, Iterator

from .incomplete_inode import IncompleteInode, IncompleteDir

_SELINUX_XATTR = b'security.selinux'


class SELinuxXAttrStats:
    'Finds the most common (and therefore likely the default) SELinux context'

    def __init__(self, inodes: Iterator[Union['Inode', IncompleteInode]]):
        self.counter = Counter(
            ino.xattrs[_SELINUX_XATTR]
                for ino in inodes if _SELINUX_XATTR in ino.xattrs
        )

    def most_common(self) -> Optional[bytes]:
        return max(
            self.counter.items(), key=lambda p: p[1], default=(None, 0),
        )[0]


def erase_mode_and_owner(
    ino: IncompleteInode, *, owner: 'InodeOwner', file_mode: int, dir_mode: int
):
    if ino.owner == owner:
        ino.owner = None
    if ((ino.mode == dir_mode) if isinstance(ino, IncompleteDir)
            else (ino.mode == file_mode)):
        ino.mode = None


def erase_utimes_in_range(
    ino: IncompleteInode, start: Tuple[int, int], end: Tuple[int, int],
):
    if ino.utimes is not None and all(start <= t <= end for t in (
        ino.utimes.ctime, ino.utimes.mtime, ino.utimes.atime,
    )):
        ino.utimes = None


def erase_selinux_xattr(ino: IncompleteInode, data: Optional[bytes]):
    if ino.xattrs.get(_SELINUX_XATTR) == data and data is not None:
        # Getting coverage for this line would force us to have a hard
        # dependency on running this test on an SELinux-enabled filesystem.
        # Mocking that seems like useless effort, so let's waive coverage.
        del ino.xattrs[_SELINUX_XATTR]  # pragma: no cover
