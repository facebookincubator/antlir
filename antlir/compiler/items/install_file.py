#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import os
import shutil
import stat
from typing import Iterable, NamedTuple, Union

from antlir.bzl.image.feature.install import install_files_t

from antlir.common import get_logger
from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesFile,
    RequireDirectory,
    RequireGroup,
    RequireUser,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol
from pydantic import PrivateAttr

from .common import ImageItem, LayerOpts, make_path_normal_relative
from .stat_options import build_stat_options

logger = get_logger()


# Default permissions, must match the docs in `install.bzl`.
_DIR_MODE = 0o755  # u+rwx,og+rx
_EXE_MODE = 0o555  # a+rx
_DATA_MODE = 0o444  # a+r


class _InstallablePath(NamedTuple):
    source: Path
    provides: Union[ProvidesDirectory, ProvidesFile]
    mode: int


def _recurse_into_source(
    source_dir: Path,
    dest_dir: Path,
    *,
    dir_mode: int,
    exe_mode: int,
    data_mode: int,
) -> Iterable[_InstallablePath]:
    "Yields paths in top-down order, making recursive copying easy."
    yield _InstallablePath(
        source=source_dir,
        provides=ProvidesDirectory(path=dest_dir),
        mode=dir_mode,
    )
    with os.scandir(source_dir) as it:
        for e in it:
            source = source_dir / e.name
            dest = dest_dir / e.name
            if e.is_dir(follow_symlinks=False):
                yield from _recurse_into_source(
                    source,
                    dest,
                    dir_mode=dir_mode,
                    exe_mode=exe_mode,
                    data_mode=data_mode,
                )
            elif e.is_file(follow_symlinks=False):
                yield _InstallablePath(
                    source=source,
                    provides=ProvidesFile(path=dest),
                    # Same `os.access` rationale as in `customize_fields`.
                    mode=exe_mode if os.access(source, os.X_OK) else data_mode,
                )
            else:
                raise RuntimeError(f"{source}: neither a file nor a directory")


def _copy_file_reflink(
    src: str, dst: str, follow_symlinks: bool = False
) -> None:
    # Use copy_file_range to get that sweet BTRFS CoW
    # We don't have to check for symlinks or collisions, since the compiler
    # will already have verified that neither of those conditions are
    # possible
    with open(src, "rb") as src_f, open(dst, "wb") as dst_f:
        remaining_len = os.fstat(src_f.fileno()).st_size
        while remaining_len > 0:
            try:
                copied = os.copy_file_range(
                    src_f.fileno(), dst_f.fileno(), remaining_len
                )
            # On older kernels (before 5.3), copy_file_range does not
            # automatically fall back to copying bytes if CoW is unavailable
            # (such as cross-filesystem copies), so we can explicitly fallback
            # to sendfile instead
            except OSError as ose:  # pragma: no cover
                if ose.errno == errno.EXDEV:
                    logger.warning(
                        "copy_file_range does not appear to support "
                        "cross-fs copies, falling back on sendfile"
                    )
                    copied = os.sendfile(
                        dst_f.fileno(),
                        src_f.fileno(),
                        offset=None,
                        count=remaining_len,
                    )
                else:
                    raise
            remaining_len -= copied


# Future enhancement notes:
#
#  (1) If we ever need to support layer sources, generalize
#      `PhasesProvideItem` -- we would need to do the same traversal,
#      but disallowing non-regular files.
# pyre-fixme[13]: Attribute `source` is never initialized.
class InstallFileItem(install_files_t, ImageItem):

    source: Path

    _paths: Iterable[_InstallablePath] = PrivateAttr()

    def __init__(self, **kwargs) -> None:
        source = kwargs["source"]
        dest = Path(make_path_normal_relative(kwargs.pop("dest")))

        # The 3 separate `*_mode` arguments must be set instead of `mode` for
        # directory sources.
        popped_args = ["mode", "exe_mode", "data_mode", "dir_mode"]
        mode, dir_mode, exe_mode, data_mode = (
            kwargs.pop(a, None) for a in popped_args
        )

        st_source = os.stat(str(source), follow_symlinks=False)
        if stat.S_ISDIR(st_source.st_mode):
            assert mode is None, "Cannot use `mode` for directory sources."
            self._paths = tuple(
                _recurse_into_source(
                    source,
                    dest,
                    dir_mode=dir_mode or _DIR_MODE,
                    exe_mode=exe_mode or _EXE_MODE,
                    data_mode=data_mode or _DATA_MODE,
                )
            )
        elif stat.S_ISREG(st_source.st_mode):
            assert {dir_mode, exe_mode, data_mode} == {
                None
            }, "Cannot use `{dir,exe,data}_mode` for file sources."
            if mode is None:
                # This tests whether the build repo user can execute the
                # file.  This is a very natural test for build artifacts,
                # and files in the repo.  Note that this can be affected if
                # the ambient umask is pathological, which is why
                # `compiler.py` checks the umask.
                mode = _EXE_MODE if os.access(source, os.X_OK) else _DATA_MODE
            self._paths = (
                _InstallablePath(
                    source=source, provides=ProvidesFile(path=dest), mode=mode
                ),
            )
        else:
            raise RuntimeError(
                f"{source} must be a regular file or directory, got {st_source}"
            )

        super().__init__(dest=dest, **kwargs)

    def provides(self):
        for i in self._paths:
            yield i.provides

    def requires(self):
        yield RequireDirectory(path=self.dest.dirname())
        yield RequireUser(self.user)
        yield RequireGroup(self.group)

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        dest = subvol.path(self.dest)

        if stat.S_ISDIR(os.stat(self.source).st_mode):
            shutil.copytree(
                str(self.source),
                str(dest),
                copy_function=_copy_file_reflink,
            )
        else:
            _copy_file_reflink(str(self.source), str(dest))

        build_stat_options(
            self,
            subvol,
            dest,
            do_not_set_mode=True,
            build_appliance=layer_opts.build_appliance,
        )

        for i in self._paths:
            os.chmod(subvol.path(i.provides.path()), i.mode)
