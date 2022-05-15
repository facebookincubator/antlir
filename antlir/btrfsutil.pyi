# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Partially-complete list of type hints for btrfsutil.

Add any more functions here as they need to be used (see `pydoc3 btrfsutil` for
the upstream module docs)
"""

from typing import Iterable, Optional, Tuple, Union

from antlir.fs_utils import Path
from antlir.unshare import Unshare

class SubvolumeInfo(object):
    id: int
    parent_id: int
    uuid: bytes
    parent_uuid: bytes

def SubvolumeIterator(
    path: Path, top: int = 0, info: bool = False, post_order: bool = False
) -> Iterable[Tuple[Path, Union[int, SubvolumeInfo]]]: ...

# we don't care about these
class QGroupInherit(object):
    pass

class BtrfsUtilError(Exception):
    errno: int

def create_snapshot(
    source: Path,
    path: Path,
    recursive: bool = False,
    read_only: bool = False,
    async_: bool = False,
    qgroup_inherit: Optional[QGroupInherit] = None,
    in_namespace: Optional[Unshare] = None,
) -> None: ...
def create_subvolume(
    path: Path,
    async_: bool = False,
    qgroup_inherit: Optional[QGroupInherit] = None,
    in_namespace: Optional[Unshare] = None,
) -> None: ...
def delete_subvolume(
    path: Path,
    recursive: bool = False,
    in_namespace: Optional[Unshare] = None,
) -> None: ...
def is_subvolume(
    path: Path,
    in_namespace: Optional[Unshare] = None,
) -> bool: ...
def set_subvolume_read_only(
    path: Path, ro: bool, in_namespace: Optional[Unshare] = None
) -> None: ...
def subvolume_id(
    path: Path,
    in_namespace: Optional[Unshare] = None,
) -> int: ...
def subvolume_info(
    path: Path,
    in_namespace: Optional[Unshare] = None,
) -> SubvolumeInfo: ...
def sync(
    path: Path,
    in_namespace: Optional[Unshare] = None,
) -> None: ...
def set_default_subvolume(
    path: Path, id: int, in_namespace: Optional[Unshare] = None
) -> None: ...
