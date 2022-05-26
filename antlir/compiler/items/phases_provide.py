#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This item is special, in that it cannot be specified from `.bzl` files, and
is ONLY injected by `dep_graph.py` in order to capture the state of the
subvolume after all the phases have finished executing, in order to
`provide()` whatever was created during the phases to the dependency sorter.
"""
import itertools
import subprocess
from collections import defaultdict
from dataclasses import dataclass
from typing import Generator

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
    ProvidesPath,
    ProvidesSymlink,
    ProvidesUser,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol

from .common import ImageItem, is_path_protected, protected_path_set
from .group import GROUP_FILE_PATH, GroupFile
from .user import PASSWD_FILE_PATH, PasswdFile


def gen_subvolume_subtree_provides(
    subvol: Subvol, subtree: Path
) -> Generator[ProvidesPath, None, None]:
    'Yields "Provides" instances for a path `subtree` in `subvol`.'
    # "Provides" classes use image-absolute paths that are `str` (for now).
    # Accept any string type to ease future migrations.
    # pyre-fixme[9]: subtree has type `Path`; used as `bytes`.
    subtree = b"/" + subtree

    protected_paths = protected_path_set(subvol)
    for prot_path in protected_paths:
        rel_to_subtree = (b"/" / prot_path).relpath(subtree)
        if not rel_to_subtree.has_leading_dot_dot():
            yield ProvidesDoNotAccess(path=rel_to_subtree)

    subtree_full_path = subvol.path(subtree)
    subtree_exists = False

    filetype_to_relpaths = defaultdict(list)
    # Traverse the subvolume as root, so that we have permission to access
    # everything.
    for type_and_path in subvol.run_as_root(
        [
            # -P is the analog of --no-dereference in GNU tools
            #
            # Filter out the protected paths at traversal time.  If one of the
            # paths has a very large or very slow mount, traversing it would
            # have a devastating effect on build times, so let's avoid looking
            # inside protected paths entirely.  An alternative would be to
            # `send` and to parse the sendstream, but this is ok too.
            "find",
            "-P",
            subtree_full_path,
            "(",
            *itertools.dropwhile(
                lambda x: x == "-o",  # Drop the initial `-o`
                itertools.chain.from_iterable(
                    [
                        # `normpath` removes the trailing / for protected dirs
                        "-o",
                        "-path",
                        subvol.path(p.normpath()),
                    ]
                    for p in protected_paths
                ),
            ),
            ")",
            "-prune",
            "-o",
            "-printf",
            "%y %p\\0",
        ],
        stdout=subprocess.PIPE,
    ).stdout.split(b"\0"):
        if not type_and_path:  # after the trailing \0
            continue
        filetype_bytes, abspath = type_and_path.split(b" ", 1)
        relpath = Path(abspath).relpath(subtree_full_path)

        assert not relpath.has_leading_dot_dot(), (
            abspath,
            subtree_full_path,
        )
        # We already "provided" this path above, and it should have been
        # filtered out by `find`.
        assert not is_path_protected(relpath, protected_paths), relpath

        if relpath == b".":
            subtree_exists = True

        filetype = filetype_bytes.decode()
        filetype_to_relpaths[filetype].append(relpath)

    for filetype, relpaths in filetype_to_relpaths.items():
        if filetype in ["b", "c", "p", "f", "s"]:
            yield from [ProvidesFile(path=r) for r in relpaths]
        elif filetype == "d":
            yield from [ProvidesDirectory(path=r) for r in relpaths]
        elif filetype == "l":
            symlink_paths = [str(subtree_full_path / r) for r in relpaths]

            # xargs --null means each input line needs to be delimited by \0
            # readlink --zero means each output line ends with \0 instead of \n
            readlink_vals = subvol.run_as_root(
                ["xargs", "--null", "readlink", "--zero"],
                stdout=subprocess.PIPE,
                input="\0".join(symlink_paths),
                text=True,
            ).stdout.split("\0")[:-1]

            assert len(relpaths) == len(readlink_vals), (
                relpaths,
                readlink_vals,
            )
            yield from [
                ProvidesSymlink(path=relpath, target=Path(readlink_val))
                for relpath, readlink_val in zip(relpaths, readlink_vals)
            ]
        else:  # pragma: no cover
            # pyre-fixme[61]: `abspath` may not be initialized here.
            raise AssertionError(f"Unknown {filetype} for {abspath}")

    # We should've gotten a CalledProcessError from `find`.
    assert subtree_exists, f"{subtree} does not exist in {subvol.path()}"


@dataclass(init=False, frozen=True)
# pyre-fixme[13]: Attribute `subvol` is never initialized.
class PhasesProvideItem(ImageItem):
    subvol: Subvol

    def provides(self):
        for path in gen_subvolume_subtree_provides(self.subvol, Path("/")):
            yield path

        # Note: Here we evaluate if the passwd/group file is available
        # in the subvol to emit users and groups.  If the db files are not
        # available we default to emitting a "root" user/group because that
        # is by default always available.  This is useful for image layers
        # that are not full OS images, but instead a "data only" layer.  The
        # limitation is that "data only" layer files must always be owned by
        # "root:root".
        group_file_path = self.subvol.path(GROUP_FILE_PATH)
        if group_file_path.exists():
            for provide in GroupFile(group_file_path.read_text()).provides():
                yield provide
        else:
            yield ProvidesGroup("root")

        passwd_file_path = self.subvol.path(PASSWD_FILE_PATH)
        if passwd_file_path.exists():
            for provide in PasswdFile(passwd_file_path.read_text()).provides():
                yield provide
        else:
            yield ProvidesUser("root")

    def requires(self):
        return ()
