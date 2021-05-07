#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.compiler.requires_provides import RequireDirectory
from antlir.fs_utils import CP_CLONE_CMD, Path
from antlir.subvol_utils import Subvol
from pydantic import root_validator

from .clone_t import clone_t
from .common import (
    ImageItem,
    LayerOpts,
    validate_path_field_normal_relative,
)
from .phases_provide import gen_subvolume_subtree_provides


class CloneItem(clone_t, ImageItem):
    class Config:
        arbitrary_types_allowed = True

    source: Path
    source_layer: Subvol

    _normalize_dest = validate_path_field_normal_relative("dest")

    @root_validator
    def check_flags(cls, values):  # noqa B902
        # Validators are classmethods but flake8 doesn't catch that.

        # This is already checked in `clone.bzl`
        assert not values["omit_outer_dir"] or values["pre_existing_dest"]
        return values

    def provides(self):
        img_rel_src = self.source.relpath(self.source_layer.path())
        assert not img_rel_src.has_leading_dot_dot(), (
            self.source,
            self.source_layer.path(),
        )
        for p in gen_subvolume_subtree_provides(self.source_layer, img_rel_src):
            if self.omit_outer_dir and p.path() == b"/":
                continue
            rel_to_src = p.path().strip_leading_slashes()
            if not self.omit_outer_dir and self.pre_existing_dest:
                rel_to_src = img_rel_src.basename() / rel_to_src
            yield p.with_new_path(self.dest / rel_to_src)

    def requires(self):
        yield RequireDirectory(
            path=self.dest if self.pre_existing_dest else self.dest.dirname()
        )

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        # The compiler should have caught this, this is just paranoia.
        if self.pre_existing_dest:
            subvol.run_as_root(["test", "-d", subvol.path(self.dest)])
        if self.omit_outer_dir:
            # Like `ls`, but NUL-separated.  Needs `root` since the repo
            # user may not be able to access the source subvol.
            sources = [
                self.source / p
                for p in subvol.run_as_root(
                    [
                        "find",
                        self.source,
                        "-mindepth",
                        "1",
                        "-maxdepth",
                        "1",
                        "-printf",
                        "%f\\0",
                    ],
                    stdout=subprocess.PIPE,
                )
                .stdout.strip(b"\0")
                .split(b"\0")
            ]
        else:
            sources = [self.source]
        # Option rationales:
        #   - The compiler should have detected any collisons on the
        #     destination, so `--no-clobber` is just a failsafe.
        #   - `--no-dereference` is needed since our contract is to copy
        #     each symlink's destination text verbatim.  Not doing this
        #     would also risk following absolute symlinks, reaching OUTSIDE
        #     of the source subvolume!
        #   - `--reflink=always` aids efficiency and, more importantly,
        #     preserves "cloned extent" relationships that existed within
        #     the source subtree.
        #   - `--sparse=auto` is implied by `--reflink=always`. The two
        #     together ought to preserve the original sparseness layout,
        #   - `--preserve=all` keeps as much original metadata as possible,
        #     including hardlinks.
        subvol.run_as_root(
            [
                *CP_CLONE_CMD,
                *sources,
                subvol.path(self.dest),
            ]
        )
