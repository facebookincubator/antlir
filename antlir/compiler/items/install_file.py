#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import itertools
import os
import stat
from typing import Iterable, NamedTuple, Optional, Union

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesFile,
    RequireDirectory,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol
from pydantic import PrivateAttr

from .common import ImageItem, LayerOpts, make_path_normal_relative
from .install_files_t import install_files_t
from .stat_options import (
    Mode,
    build_stat_options,
    customize_stat_options,
    mode_to_str,
)


# Default permissions, must match the docs in `install.bzl`.
_DIR_MODE = "u+rwx,og+rx"
_EXE_MODE = "a+rx"
_DATA_MODE = "a+r"


class _InstallablePath(NamedTuple):
    source: Path
    provides: Union[ProvidesDirectory, ProvidesFile]
    mode: Mode


def _recurse_into_source(
    source_dir: Path,
    dest_dir: Path,
    *,
    dir_mode: Mode,
    exe_mode: Mode,
    data_mode: Mode,
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


# Future enhancement notes:
#
# (1) Although `install_buck_runnable` does not support handling
#     directories at present, this is not explicitly checked here.  What
#     I expect to happen if somebody does try installing a `buck
#     run`nable directory is that it will build, but any in-place
#     executables inside that directory will not work in @mode/dev.  If
#     a need to fix this comes up, the fix would involve the `.bzl` code
#     in @mode/dev passing to us (i) the source, as today, (ii) the
#     wrapped source using a wrapper with `dynamic_path_in_output=True`.
#     This code here would just need a branch to convert all the
#     executable files to use the wrapper.  Hint 1: Review the long
#     comment in D16042669 for a discussion of the details.  Hint 2:
#     look at mentions of `is_buck_runnable_` in D18905604 for a list of
#     locations, where you would need to add the "wrapper path" field.
#
#  (2) If we ever need to support layer sources, generalize
#      `PhasesProvideItem` -- we would need to do the same traversal,
#      but disallowing non-regular files.
class InstallFileItem(install_files_t, ImageItem):

    source: Path

    _paths: Optional[Iterable[_InstallablePath]] = PrivateAttr()

    def __init__(self, **kwargs):
        customize_stat_options(kwargs, default_mode=None)  # Defaulted later

        source = kwargs["source"]
        dest = Path(make_path_normal_relative(kwargs.pop("dest")))

        # The 3 separate `*_mode` arguments must be set instead of `mode` for
        # directory sources.
        popped_args = ["mode", "exe_mode", "data_mode", "dir_mode"]
        mode, dir_mode, exe_mode, data_mode = (
            kwargs.pop(a, None) for a in popped_args
        )

        st_source = os.stat(source, follow_symlinks=False)
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

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        dest = subvol.path(self.dest)
        # The compiler should have detected any collisons, so `--no-clobber`
        # is just a failsafe.  `--no-dereference` is also a failsafe since
        # we ban symlinks above.
        #
        # Opportunistic reflinking & mandatory sparsification are easy
        # efficiency wins.
        #
        # Don't bother preserving metadata since we explicitly set mode &
        # ownership ...  and our build setup lets timestamp float (for now).
        subvol.run_as_root(
            [
                "cp",
                "--recursive",
                "--no-clobber",
                "--no-dereference",
                "--reflink=auto",
                "--sparse=always",
                "--no-preserve=all",
                self.source,
                dest,
            ]
        )
        build_stat_options(self, subvol, dest, do_not_set_mode=True)
        # Group by mode to make as few shell calls as possible.
        for mode_str, modes_and_paths in itertools.groupby(
            sorted(
                (mode_to_str(i.mode), i.provides.path()) for i in self._paths
            ),
            lambda x: x[0],
        ):
            # Batching chmod calls has the unfortunate side effect of failing
            # on installing large amounts of files with one `image.install`.
            # xargs will break up the command for us if we overrun the maximum
            # args size limit.
            # `chmod` follows symlinks, and there's no option to stop it.
            # However, `customize_fields` should have failed on symlinks.
            paths = b"\0".join(subvol.path(p) for _, p in modes_and_paths)
            subvol.run_as_root(
                ["xargs", "-0", "chmod", mode_str],
                input=paths,
            )
