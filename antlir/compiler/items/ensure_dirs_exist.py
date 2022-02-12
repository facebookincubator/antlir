#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd
import subprocess
from typing import Iterator, Optional

from antlir.bzl.image_actions.ensure_subdirs_exist import ensure_subdirs_exist_t
from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    RequireDirectory,
    RequireUser,
    RequireGroup,
)
from antlir.fs_utils import Path, generate_work_dir
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol
from pydantic import root_validator, validator

from .common import (
    ImageItem,
    LayerOpts,
    make_path_normal_relative,
)
from .stat_options import (
    Mode,
    build_stat_options,
    customize_stat_options,
    mode_to_octal_str,
)


class MismatchError(Exception):
    pass


_BUILD_SCRIPT = r"""
path_to_make="$1"
expected_stat="$2"
if  [ -d "$path_to_make" ]; then
    stat_res="$(stat --format="0%a %U:%G" "$path_to_make")"
    if [ "$stat_res" != "$expected_stat" ]; then
        echo "ERROR: stat did not match \"$expected_stat\" for $path_to_make: $stat_res"
        exit 1
    fi
    xattrs_res="$(getfattr -m '-' -d --absolute-names "$path_to_make" | (grep -vE '^(# file: |security\.selinux=|$)' || :))"
    if [ -n "$xattrs_res" ]; then
        echo "ERROR: xattrs was not empty for $path_to_make: $xattrs_res"
        exit 1
    fi
else
    mkdir "$path_to_make"
fi
"""  # noqa: E501


def _validate_into_dir(into_dir: Optional[str]) -> str:
    if into_dir == "":
        raise ValueError('`into_dir` was the empty string; for root, use "/"')
    # pyre-fixme[7]: Expected `str` but got `Optional[str]`.
    return into_dir


# `ensure_subdirs_exist_factory` below should be used to construct this
# pyre-fixme[13]: Attribute `basename` is never initialized.
# pyre-fixme[13]: Attribute `subdirs_to_create` is never initialized.
class EnsureDirsExistItem(ensure_subdirs_exist_t, ImageItem):
    basename: str
    mode: Optional[Mode]

    # `subdirs_to_create` is an `ensure_subdirs_exist_t` shape field that is
    # processed by `ensure_subdirs_exist_factory` below to create an
    # `EnsureDirsExistItem` item for each subdir level. This field is
    # required in the shape, but should never be provided to this item. Thus,
    # we've overridden the field to be Optional and assert that it is None
    # in the validator below. Alternatively, we could remove the field, but
    # that is unnatural (both conceptually and in implementation).
    #
    # NB: `ensure_subdirs_exist_factory` breaks up the incoming item config
    # to resolve cicular dependencies and allow for a cleaner dependency
    # graph. More info available in the factory function's docstring.
    # pyre-fixme[15]: `subdirs_to_create` overrides attribute defined in
    #  `ensure_subdirs_exist_t` inconsistently.
    subdirs_to_create: Optional[str]

    @validator("subdirs_to_create")
    def validate_subdirs_to_create(cls, subdirs_to_create):  # noqa B902
        # subdirs_to_create should only exist on the config args being
        # passed to `ensure_subdirs_exist_factory` and must not be
        # passed to EnsureDirsExistItem.
        raise AssertionError(subdirs_to_create)

    @validator("into_dir")
    def validate_into_dir(cls, into_dir) -> str:  # noqa B902
        # Validators are classmethods but flake8 doesn't catch that.
        return make_path_normal_relative(_validate_into_dir(into_dir))

    @validator("basename")
    def validate_basename(cls, basename: str) -> str:  # noqa B902
        basename = make_path_normal_relative(basename)
        # We want this to be a single path component (the dir being made)
        assert "/" not in basename
        return basename

    @root_validator
    def set_default_stat_options(cls, values):  # noqa B902
        customize_stat_options(values, default_mode=0o755)
        return values

    def provides(self):
        yield ProvidesDirectory(path=Path(self.into_dir) / self.basename)

    def requires(self):
        yield RequireDirectory(path=Path(self.into_dir))
        user, group = self.user_group.split(":")
        yield RequireUser(user)
        yield RequireGroup(group)

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        # If path already exists ensure it has expected attrs, else make it.
        work_dir = generate_work_dir()
        full_path = work_dir / self.into_dir / self.basename
        path_to_make = subvol.path() / self.into_dir / self.basename
        # Cannot postpone exists() check because _BUILD_SCRIPT will create the
        # directory `path_to_make`
        path_to_make_exists = path_to_make.exists()
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
                # pyre-fixme[6]: Expected `Union[int, str]` for 1st param but
                #  got `Union[None, int, str]`.
                f"{mode_to_octal_str(self.mode)} {self.user_group}",
            ],
            layer=layer_opts.build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam("root"),
        )
        try:
            run_nspawn(opts, PopenArgs())
        except subprocess.CalledProcessError as e:
            raise MismatchError(
                f"Failed to ensure_subdirs_exist for path '{full_path}' with"
                # pyre-fixme[6]: Expected `Union[int, str]` for 1st param but
                #  got `Union[None, int, str]`.
                f" stat {mode_to_octal_str(self.mode)}"
                ": see bash error above for more details",
            ) from e
        if not path_to_make_exists:
            build_stat_options(
                self,
                subvol,
                # pyre-fixme[6]: Expected `str` for 3rd param but got `Path`.
                path_to_make,
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
        feature.ensure_dir_symlink("/x/y", "/a/b/c/d"),
    ```

    Here, `ensure_dir_symlink` requires dirs "/x/y" and "/a/b/c" and provides
    "/a/b/c/d". If `ensure_dirs_exist` were a single item, it would provide
    paths "/a", "/a/b", "/a/b/c", "/a/b/c/d". This means `ensure_dir_symlink`
    requires `ensure_dirs_exist` (e.g. for path "/a/b/c"), but
    `ensure_dirs_exist` also requires `ensure_dir_symlink` (for path
    "/a/b/c/d", because they both provide it, and we need to ensure
    `ensure_dirs_exist` runs last, so we make an artificial dep). Thus, we hit
    a cycle in the dep graph.

    Now, if we instead denormalize the EDE declaration into a separate item for
    each path component, we do not need to worry about the cycle, because the
    EDE providing "/a/b/c" and the EDE requiring `ensure_dir_symlink` for
    "/a/b/c/d" are separate items.
    """
    # pyre-fixme[9]: into_dir has type `str`; used as `Path`.
    into_dir = Path(make_path_normal_relative(_validate_into_dir(into_dir)))
    subdirs_to_create = make_path_normal_relative(subdirs_to_create)
    # pyre-fixme[58]: `/` is not supported for operand types `str` and `str`.
    path = into_dir / subdirs_to_create
    while True:
        parent = path.dirname()
        yield EnsureDirsExistItem(
            **kwargs,
            # Want to provide root rather than the empty string
            into_dir=parent.decode() or "/",
            basename=path.basename().decode(),
        )
        if parent == into_dir:
            break
        path = parent
