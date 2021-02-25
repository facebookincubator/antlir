#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
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
from dataclasses import dataclass

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol

from .common import ImageItem, is_path_protected, protected_path_set


def gen_subvolume_subtree_provides(subvol: Subvol, subtree: Path):
    'Yields "Provides" instances for a path `subtree` in `subvol`.'
    # "Provides" classes use image-absolute paths that are `str` (for now).
    # Accept any string type to ease future migrations.
    subtree = b"/" + subtree

    protected_paths = protected_path_set(subvol)
    for prot_path in protected_paths:
        rel_to_subtree = (b"/" / prot_path).relpath(subtree)
        if not rel_to_subtree.has_leading_dot_dot():
            yield ProvidesDoNotAccess(path=rel_to_subtree)

    subtree_full_path = subvol.path(subtree)
    subtree_exists = False
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

        # Future: This provides all symlinks as files, while we should
        # probably provide symlinks to valid directories inside the image as
        # directories to be consistent with SymlinkToDirItem.
        filetype = filetype_bytes.decode()
        if filetype in ["b", "c", "p", "f", "l", "s"]:
            yield ProvidesFile(path=relpath)
        elif filetype == "d":
            yield ProvidesDirectory(path=relpath)
        else:  # pragma: no cover
            raise AssertionError(f"Unknown {filetype} for {abspath}")
        if relpath == b".":
            subtree_exists = True

    # We should've gotten a CalledProcessError from `find`.
    assert subtree_exists, f"{subtree} does not exist in {subvol.path()}"


@dataclass(init=False, frozen=True)
class PhasesProvideItem(ImageItem):
    subvol: Subvol

    def provides(self):
        return gen_subvolume_subtree_provides(self.subvol, Path("/"))

    def requires(self):
        return ()
