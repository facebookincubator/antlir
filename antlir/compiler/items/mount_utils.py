#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Separate from `mount.py` to avoid circular dep with `common.py`"
import json
import os
from typing import Iterator

from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol


META_MOUNTS_DIR = Path(".meta/private/mount")
MOUNT_MARKER = Path("MOUNT")


# Not covering, since this would require META_MOUNTS_DIR to be unreadable.
def _raise(ex):  # pragma: no cover
    raise ex


def mountpoints_from_subvol_meta(subvol: Subvol) -> Iterator[Path]:
    """
    Returns image-relative paths to mountpoints.  Directories get a trailing
    /, while files do not.  See the `_protected_path_set` docblock if this
    convention proves onerous.
    """
    mounts_path = subvol.path(META_MOUNTS_DIR)
    if not mounts_path.exists():
        return

    for path, _next_dirs, _files in os.walk(
        # We are not `chroot`ed, so following links could access outside the
        # image; `followlinks=False` is the default -- explicit for safety.
        mounts_path,
        onerror=_raise,
        followlinks=False,
    ):
        relpath = Path(path).relpath(mounts_path)
        if relpath.basename() == MOUNT_MARKER:
            mountpoint = relpath.dirname()
            assert not mountpoint.endswith(b"/"), mountpoint
            assert not mountpoint.startswith(b"/"), mountpoint
            # It would be more technically correct to use `subvol.path()`
            # here (since that prevents us from following links outside the
            # image), but this is much more legible and probably safe.
            with open(Path(path) / "is_directory") as f:
                is_directory = json.load(f)
            yield mountpoint / "" if is_directory else mountpoint
