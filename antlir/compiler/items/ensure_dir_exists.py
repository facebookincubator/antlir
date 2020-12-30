#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
from dataclasses import dataclass
from typing import Iterator

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    require_directory,
)
from antlir.fs_utils import Path, generate_work_dir
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol

from .common import (
    ImageItem,
    LayerOpts,
    coerce_path_field_normal_relative,
    make_path_normal_relative,
)
from .stat_options import (
    Mode,
    build_stat_options,
    customize_stat_options,
    mode_to_str,
)

# TODO(jtru): Uncomment xattrs check when new BA is released
_BUILD_SCRIPT = r"""
path_to_make="$1"
expected_stat="$2"
# If dir exists ensure its attributes match expectations
if [ -d "$path_to_make" ]; then
    stat_res="$(stat --format="0%a %U:%G" "$path_to_make")"
    if [ "$stat_res" != "$expected_stat" ]; then
        echo "ERROR: stat did not match \"$expected_stat\" for $path_to_make: $stat_res"
        exit 1
    fi
    # xattrs_res="$(getfattr -m '-' -d --absolute-names "$path_to_make" | grep -v '^\(# file: \|security\.selinux=\)')"
    # if [ -n "$xattrs_res" ]; then
    #     echo "ERROR: xattrs was not empty for $path_to_make: $xattrs_res"
    #     exit 1
    # fi
else
    mkdir "$path_to_make"
fi
"""  # noqa: E501


# `ensure_dir_exists_factory` below should be used to construct these
@dataclass(init=False, frozen=True)
class EnsureDirExistsItem(ImageItem):
    into_dir: str
    basename: str

    # Stat option fields
    mode: Mode
    user_group: str

    @classmethod
    def customize_fields(cls, kwargs):
        super().customize_fields(kwargs)
        coerce_path_field_normal_relative(kwargs, "into_dir")
        # We want this to be a single path component (the dir being made)
        assert "/" not in kwargs.get("basename", "")
        # Unlike files, leave directories as writable by the owner by
        # default, since it's reasonable for files to be added at runtime.
        customize_stat_options(kwargs, default_mode=0o755)

    def provides(self):
        yield ProvidesDirectory(path=os.path.join(self.into_dir, self.basename))

    def requires(self):
        yield require_directory(self.into_dir)

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        # If path already exists ensure it has expected attrs, else make it.
        work_dir = generate_work_dir()
        full_path = Path(work_dir) / self.into_dir / self.basename
        opts = new_nspawn_opts(
            cmd=[
                "/bin/bash",
                "-eu",
                "-o",
                "pipefail",
                "-c",
                _BUILD_SCRIPT,
                "bash",
                full_path,
                f"{mode_to_str(self.mode)} {self.user_group}",
            ],
            layer=layer_opts.build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam("root"),
        )
        run_nspawn(opts, PopenArgs())
        build_stat_options(
            self,
            subvol,
            subvol.path(os.path.join(self.into_dir, self.basename)),
            build_appliance=layer_opts.build_appliance,
        )


def ensure_dir_exists_factory(
    *, path: str, **kwargs
) -> Iterator[EnsureDirExistsItem]:
    """Convenience factory to create a set of EnsureDirExistsItems. This allows
    us to provide one cohesive API for a given path and then denormalize that
    path to separate items for each path component. For example, for the given
    image feature:

        image.ensure_dir_exists('/a/b/c')

    This factory would yield:

        EnsureDirExistsItem("/", "a"),
        EnsureDirExistsItem("/a", "b"),
        EnsureDirExistsItem("/a/b", "c"),

    This separation into multiple items is a necessary step to avoid circular
    dependencies. Specifically:

    - EnsureDirExists (EDE) items take a dependency on any other item types in
        the dependency graph, to ensure they're the last items to run for a
        given path (for more info, see comments in `dep_graph.py`).
    - It's also possible that any items providing a directory may depend on an
        EDE item for another directory (see example below).
    - In this situation, if a full path were provided only by a single EDE item,
      cycles would be possible any time another item type providing directories
      also required a directory only supplied by that EDE item.

    To visualize this problem, consider the following setup:

    ```
        image.ensure_dir_exists("/a/b/c/d"),
        image.symlink_dir("/x/y", "/a/b/c/d"),
    ```

    Here, `symlink_dir` requires dirs "/x/y" and "/a/b/c" and provides
    "/a/b/c/d". If `ensure_dir_exists` were a single item, it would provide
    paths "/a", "/a/b", "/a/b/c", "/a/b/c/d". This means `symlinks_dir` requires
    `ensure_dir_exists` (e.g. for path "/a/b/c"), but `ensure_dir_exists` also
    requires `symlinks_dir` (for path "/a/b/c/d", because they both provide it,
    and we need to ensure `ensure_dir_exists` runs last, so we make an
    artificial dep). Thus, we hit a cycle in the dep graph.

    Now, if we instead denormalize the EDE declaration into a separate item for
    each path component, we do not need to worry about the cycle, because the
    EDE providing "/a/b/c" and the EDE requiring `symlink_dir` for "/a/b/c/d"
    are separate items.
    """
    curr_path = make_path_normal_relative(path)
    assert not curr_path.startswith("/")
    while True:
        into_dir = os.path.dirname(curr_path)
        yield EnsureDirExistsItem(
            **kwargs,
            into_dir=into_dir,
            basename=os.path.basename(curr_path),
        )
        if not into_dir:
            return
        curr_path = into_dir
