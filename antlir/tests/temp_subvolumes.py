#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import contextlib
import functools
import logging
import os
import sys
from typing import AnyStr

from antlir.fs_utils import Path, temp_dir

from ..find_built_subvol import volume_dir
from ..subvol_utils import Subvol


def with_temp_subvols(method):
    """
    A test that needs a TempSubvolumes instance should use this decorator.
    This is a cleaner alternative to doing this in setUp:

        self.temp_subvols.__enter__()
        self.addCleanup(self.temp_subvols.__exit__, None, None, None)

    The primary reason this is bad is explained in the TempSubvolumes
    docblock. It also fails to pass exception info to the __exit__.
    """

    @functools.wraps(method)
    def decorated(self, *args, **kwargs):
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            return method(self, temp_subvols, *args, **kwargs)

    return decorated


class TempSubvolumes(contextlib.AbstractContextManager):
    """
    Tracks the subvolumes it creates, and destroys them on context exit.

    Note that relying on unittest.TestCase.addCleanup to __exit__ this
    context is unreliable -- e.g. clean-up is NOT triggered on
    KeyboardInterrupt. Therefore, this **will** leak subvolumes
    during development. You can clean them up thus:

        sudo btrfs sub del buck-image-out/volume/tmp/TempSubvolumes_*/subvol &&
            rmdir buck-image-out/volume/tmp/TempSubvolumes_*

    Instead of polluting `buck-image-out/volume`, it  would be possible to
    put these on a separate `LoopbackVolume`, to rely on `Unshare` to
    guarantee unmounting it, and to rely on `tmpwatch` to delete the stale
    loopbacks from `/tmp/`.  At present, this doesn't seem worthwhile since
    it would require using an `Unshare` object throughout `Subvol`.

    The easier approach is to write `with TempSubvolumes() ...` in each test.
    """

    def __init__(self, path_in_repo=None):
        self.subvols = []
        # The 'tmp' subdirectory simplifies cleanup of leaked temp subvolumes
        volume_tmp_dir = os.path.join(volume_dir(path_in_repo), "tmp")
        try:
            os.mkdir(volume_tmp_dir)
        except FileExistsError:
            pass
        # Our exit is written with exception-safety in mind, so this
        # `_temp_dir_ctx` **should** get `__exit__`ed when this class does.
        self._temp_dir_ctx = temp_dir(  # noqa: P201
            dir=volume_tmp_dir, prefix=self.__class__.__name__ + "_"
        )

    def __enter__(self):
        self._temp_dir = self._temp_dir_ctx.__enter__()
        return self

    def _prep_rel_path(self, rel_path: AnyStr) -> Path:
        """
        Ensures subvolumes live under our temporary directory, which
        improves safety, since its permissions ought to be u+rwx to avoid
        exposing setuid binaries inside the built subvolumes.
        """
        rel_path = (
            (self._temp_dir / rel_path)
            .realpath()
            .relpath(self._temp_dir.realpath())
        )
        if rel_path.has_leading_dot_dot():
            raise AssertionError(
                f"{rel_path} must be a subdirectory of {self._temp_dir}"
            )
        abs_path = self._temp_dir / rel_path
        try:
            os.makedirs(abs_path.dirname())
        except FileExistsError:
            pass
        return abs_path

    def create(self, rel_path: AnyStr) -> Subvol:
        subvol = Subvol(self._prep_rel_path(rel_path))
        subvol.create()
        self.subvols.append(subvol)
        return subvol

    def snapshot(self, source: Subvol, dest_rel_path: AnyStr) -> Subvol:
        dest = Subvol(self._prep_rel_path(dest_rel_path))
        dest.snapshot(source)
        self.subvols.append(dest)
        return dest

    def caller_will_create(self, rel_path: AnyStr) -> Subvol:
        subvol = Subvol(self._prep_rel_path(rel_path))
        # If the caller fails to create it, our __exit__ is robust enough
        # to ignore this subvolume.
        self.subvols.append(subvol)
        return subvol

    def __exit__(self, exc_type, exc_val, exc_tb):
        # If any of subvolumes are nested, and the parents were made
        # read-only, we won't be able to delete them.
        for subvol in self.subvols:
            try:
                subvol.set_readonly(False)
            except BaseException:  # Ctrl-C does not interrupt cleanup
                pass
        for subvol in reversed(self.subvols):
            try:
                subvol._delete_inner_subvols()
                subvol.delete()
            except BaseException:  # Ctrl-C does not interrupt cleanup
                logging.exception(f"Deleting volume {subvol.path()} failed.")
        return self._temp_dir_ctx.__exit__(exc_type, exc_val, exc_tb)
