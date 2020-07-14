#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import enum
import os
from dataclasses import dataclass
from typing import Iterable

from fs_image.subvol_utils import Subvol

from .common import (
    ImageItem,
    LayerOpts,
    PhaseOrder,
    coerce_path_field_normal_relative,
    is_path_protected,
    protected_path_set,
)


class RemovePathAction(enum.Enum):
    assert_exists = "assert_exists"
    if_exists = "if_exists"


@dataclass(init=False, frozen=True)
class RemovePathItem(ImageItem):

    path: str
    action: RemovePathAction

    @classmethod
    def customize_fields(cls, kwargs):
        super().customize_fields(kwargs)
        coerce_path_field_normal_relative(kwargs, "path")
        kwargs["action"] = RemovePathAction(kwargs["action"])

    def phase_order(self):
        return PhaseOrder.REMOVE_PATHS

    def __sort_key(self):
        return (
            self.path,
            {
                action: idx
                for idx, action in enumerate(
                    [
                        # We sort in reverse order, so by putting "if" first we
                        # allow conflicts between "if_exists" and
                        # "assert_exists" items to be resolved naturally.
                        RemovePathAction.if_exists,
                        RemovePathAction.assert_exists,
                    ]
                )
            }[self.action],
        )

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["RemovePathItem"], layer_opts: LayerOpts
    ):
        # NB: We want `remove_paths` not to be able to remove additions by
        # regular (non-phase) items in the same layer -- that indicates
        # poorly designed `image.feature`s, which should be refactored.  At
        # present, this is only enforced implicitly, because all removes are
        # done before regular items are even validated or sorted.  Enforcing
        # it explicitly is possible by peeking at `DependencyGraph.items`,
        # but the extra complexity doesn't seem worth the faster failure.

        # NB: We could detect collisions between two `assert_exists` removes
        # early, but again, it doesn't seem worth the complexity.

        def builder(subvol: Subvol):
            protected_paths = protected_path_set(subvol)
            # Reverse-lexicographic order deletes inner paths before
            # deleting the outer paths, thus minimizing conflicts between
            # `remove_paths` items.
            for item in sorted(
                items, reverse=True, key=lambda i: i.__sort_key()
            ):
                if is_path_protected(item.path, protected_paths):
                    # For META_DIR, this is never reached because of
                    # make_path_normal_relative's check, but for other
                    # protected paths, this is required.
                    raise AssertionError(
                        f"Cannot remove protected {item}: {protected_paths}"
                    )
                # This ensures that there are no symlinks in item.path that
                # might take us outside of the subvolume.  Since recursive
                # `rm` does not follow symlinks, it is OK if the inode at
                # `item.path` is a symlink (or one of its sub-paths).
                path = subvol.path(item.path, no_dereference_leaf=True)
                if not os.path.lexists(path):
                    if item.action == RemovePathAction.assert_exists:
                        raise AssertionError(f"Path does not exist: {item}")
                    elif item.action == RemovePathAction.if_exists:
                        continue
                    else:  # pragma: no cover
                        raise AssertionError(f"Unknown {item.action}")
                subvol.run_as_root(
                    [
                        "rm",
                        "-r",
                        # This prevents us from making removes outside of the
                        # per-repo loopback, which is an important safeguard.
                        # It does not stop us from reaching into other subvols,
                        # but since those have random IDs in the path, this is
                        # nearly impossible to do by accident.
                        "--one-file-system",
                        path,
                    ]
                )
            pass

        return builder
