#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
from dataclasses import dataclass
from typing import Iterator, Optional

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
    mode_to_octal_str,
)

# TODO(jtru): Uncomment xattrs check when new BA is released
_BUILD_SCRIPT = r"""
path_to_make="$1"
expected_stat="$2"
if  [ -d "$path_to_make" ]; then
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


def _validate_into_dir(into_dir: Optional[str]) -> str:
    if into_dir == "":
        raise ValueError('`into_dir` was the empty string; for root, use "/"')
    return into_dir


# `ensure_subdirs_exist_factory` below should be used to construct this
@dataclass(init=False, frozen=True)
class EnsureDirsExistItem(ImageItem):
    into_dir: str
    basename: str

    # Stat option fields
    mode: Mode
    user_group: str

    @classmethod
    def customize_fields(cls, kwargs):
        super().customize_fields(kwargs)
        _validate_into_dir(kwargs.get("into_dir", None))
        coerce_path_field_normal_relative(kwargs, "into_dir")
        coerce_path_field_normal_relative(kwargs, "basename")
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
                f"{mode_to_octal_str(self.mode)} {self.user_group}",
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


def ensure_subdirs_exist_factory(
    *, into_dir: str, subdirs_to_create: str, **kwargs
) -> Iterator[EnsureDirsExistItem]:
    """Convenience factory to create a set of EnsureDirsExistItems. This allows
    us to provide a single API for the creation of a given directory, and then
    denormalize that path to separate items for each path component.

    Specifically, this provides the ability for users to specify subdirs to
    create within a directory that's known to exist. This is useful if the
    caller would like the attributes of subdirs to differ from those of their
    parents.

    This denormalization of a path to separate items is a critical to avoid
    circular dependencies. For example, for the given image feature:

        image.ensure_dirs_exist("/a/b/c")

    This factory would yield:

        EnsureDirsExistItem("/", "a"),
        EnsureDirsExistItem("/a", "b"),
        EnsureDirsExistItem("/a/b", "c"),

    In the above, it's worth noting:

    - EnsureDirsExist (EDE) items take a dependency on any other item types in
        the dependency graph, to ensure they're the last items to run for a
        given path (for more info, see comments in `dep_graph.py`).
    - It's also possible that any items providing a directory may depend on an
        EDE item for another directory (see example below).
    - In this situation, if a full path were provided only by a single EDE item,
        cycles would be possible any time another item type providing
        directories also required a directory only supplied by that EDE item.

    To visualize this problem, consider the following setup:

    ```
        image.ensure_dirs_exist("/a/b/c/d"),
        image.symlink_dir("/x/y", "/a/b/c/d"),
    ```

    Here, `symlink_dir` requires dirs "/x/y" and "/a/b/c" and provides
    "/a/b/c/d". If `ensure_dirs_exist` were a single item, it would provide
    paths "/a", "/a/b", "/a/b/c", "/a/b/c/d". This means `symlinks_dir` requires
    `ensure_dirs_exist` (e.g. for path "/a/b/c"), but `ensure_dirs_exist` also
    requires `symlinks_dir` (for path "/a/b/c/d", because they both provide it,
    and we need to ensure `ensure_dirs_exist` runs last, so we make an
    artificial dep). Thus, we hit a cycle in the dep graph.

    Now, if we instead denormalize the EDE declaration into a separate item for
    each path component, we do not need to worry about the cycle, because the
    EDE providing "/a/b/c" and the EDE requiring `symlink_dir` for "/a/b/c/d"
    are separate items.
    """
    into_dir = make_path_normal_relative(_validate_into_dir(into_dir))
    subdirs_to_create = make_path_normal_relative(subdirs_to_create)
    path = os.path.join(into_dir, subdirs_to_create)
    while True:
        parent = os.path.dirname(path)
        yield EnsureDirsExistItem(
            **kwargs,
            # Want to provide root rather than the empty string
            into_dir=parent or "/",
            basename=os.path.basename(path),
        )
        if parent == into_dir:
            break
        path = parent
