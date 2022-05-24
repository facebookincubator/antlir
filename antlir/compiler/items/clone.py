#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.bzl.image_actions.clone import clone_t
from antlir.compiler.requires_provides import RequireDirectory
from antlir.fs_utils import CP_CLONE_CMD, Path
from antlir.subvol_utils import Subvol
from pydantic import root_validator

from .common import ImageItem, LayerOpts, validate_path_field_normal_relative
from .phases_provide import gen_subvolume_subtree_provides


# pyre-fixme[13]: Attribute `source` is never initialized.
# pyre-fixme[13]: Attribute `source_layer` is never initialized.
class CloneItem(clone_t, ImageItem):
    class Config:
        arbitrary_types_allowed = True

    source: Path
    # pyre-fixme[15]: `source_layer` overrides attribute defined in `clone_t`
    #  inconsistently.
    source_layer: Subvol

    # pyre-fixme[4]: Attribute must be annotated.
    _normalize_dest = validate_path_field_normal_relative("dest")

    @root_validator
    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def check_flags(cls, values):  # noqa B902
        # Validators are classmethods but flake8 doesn't catch that.

        # This is already checked in `clone.bzl`
        assert not values["omit_outer_dir"] or values["pre_existing_dest"]
        return values

    # pyre-fixme[3]: Return type must be annotated.
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

    # pyre-fixme[3]: Return type must be annotated.
    def requires(self):
        yield RequireDirectory(
            path=self.dest if self.pre_existing_dest else self.dest.dirname()
        )

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        # The compiler should have caught this, this is just paranoia.
        if self.pre_existing_dest:
            subprocess.run(["test", "-d", subvol.path(self.dest)], check=True)
        if self.omit_outer_dir:
            # Like `ls`, but NUL-separated.  Needs `root` since the repo
            # user may not be able to access the source subvol.
            sources = [
                self.source / p
                for p in subprocess.run(
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
                    check=True,
                    stdout=subprocess.PIPE,
                )
                .stdout.strip(b"\0")
                .split(b"\0")
            ]
        else:
            sources = [self.source]

        subprocess.run(
            [*CP_CLONE_CMD, *sources, subvol.path(self.dest)],
            check=True,
        )
