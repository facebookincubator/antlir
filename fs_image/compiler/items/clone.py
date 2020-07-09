#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from dataclasses import dataclass

from fs_image.compiler.requires_provides import require_directory
from fs_image.fs_utils import Path
from fs_image.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, coerce_path_field_normal_relative
from .phases_provide import gen_subvolume_subtree_provides


@dataclass(init=False, frozen=True)
class CloneItem(ImageItem):

    dest: str
    omit_outer_dir: bool
    pre_existing_dest: bool

    source: Path
    source_layer: Subvol

    @classmethod
    def customize_fields(cls, kwargs):
        super().customize_fields(kwargs)
        # This is already checked in `clone.bzl`
        assert not kwargs["omit_outer_dir"] or kwargs["pre_existing_dest"]
        coerce_path_field_normal_relative(kwargs, "dest")

    def provides(self):
        img_rel_src = self.source.relpath(self.source_layer.path())
        assert not img_rel_src.has_leading_dot_dot(), (
            self.source,
            self.source_layer.path(),
        )
        for p in gen_subvolume_subtree_provides(self.source_layer, img_rel_src):
            if self.omit_outer_dir and p.path == "/":
                continue
            rel_to_src = p.path.lstrip("/")
            if not self.omit_outer_dir and self.pre_existing_dest:
                rel_to_src = os.path.join(
                    img_rel_src.basename().decode(), rel_to_src
                )
            yield p.with_new_path(os.path.join(self.dest, rel_to_src))

    def requires(self):
        yield require_directory(
            self.dest if self.pre_existing_dest else os.path.dirname(self.dest)
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
                "cp",
                "--recursive",
                "--no-clobber",
                "--no-dereference",
                "--reflink=always",
                "--sparse=auto",
                "--preserve=all",
                *sources,
                subvol.path(self.dest),
            ]
        )
