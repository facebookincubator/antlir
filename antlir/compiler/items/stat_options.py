#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Helpers for setting `stat (2)` options on files, directories, etc, which
we are creating inside the image.
"""

import os
import pwd
from typing import Union

from antlir.fs_utils import generate_work_dir
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol


# `mode` can be an integer fully specifying the bits, or a chmod symbolic string
# like `u+rx`.  In the latter case, the changes are applied on top of mode 0.
STAT_OPTION_FIELDS = [("mode", None), ("user_group", None)]

Mode = Union[str, int]  # human-readable chmod symbolic string, or octal

_STAT_CLASSES = {
    "u": lambda b: b << 6,
    "g": lambda b: b << 3,
    "o": lambda b: b,
    "a": lambda b: b << 6 | b << 3 | b,
}
_STAT_PERMS = {
    "r": 0b100,
    "w": 0b010,
    "x": 0b001,
    # These are handled separately
    "s": 0b000,
    "t": 0b000,
}
# Handle sticky and "set on execution" bits separately as they only apply to
# certain classes, and are always applied to the 3 leftmost bits
_STAT_EXTRA_PERMS = {
    ("s", "u"): 0b100,
    ("s", "g"): 0b010,
    ("s", "a"): 0b110,
    ("t", "a"): 0b001,
}


def customize_stat_options(kwargs, *, default_mode):
    "Mutates `kwargs`."
    if kwargs.get("mode") is None:
        kwargs["mode"] = default_mode
    if kwargs.get("user_group") is None:
        kwargs["user_group"] = "root:root"


def mode_to_int(mode: Mode) -> int:
    if isinstance(mode, int):
        return mode
    elif isinstance(mode, str):
        mode = mode_to_octal_str(mode)
        return int(mode, 8)
    else:
        raise TypeError(f"{mode} was neither an int nor str")


def mode_to_octal_str(mode: Mode) -> str:
    """Converts an instance of `Mode` to an octal string. If `mode` is a string,
    it's expected to be in the chmod symbolic string format with the following
    added restrictions:

    - Only append ("+") actions are supported, as we always apply the changes on
      top of mode 0.
    - No "X" is supported, as this conversion must be compatible with `stat(1)`.
    """
    # `mode` can be the empty string
    mode = mode or 0
    if isinstance(mode, int):
        return f"{mode:04o}"
    assert (
        "-" not in mode and "=" not in mode
    ), "Only append actions ('+') are supported in mode strings"
    result = 0
    for directive in mode.split(","):
        try:
            classes, perms = directive.split("+")
        except ValueError:
            raise ValueError(
                "Expected directive in the form [classes...]+[perms...] "
                f"for {mode}"
            )
        # Support empty classes
        classes = classes or ["a"]
        for stat_cls in classes:
            stat_cls_fn = _STAT_CLASSES.get(stat_cls, None)
            assert stat_cls_fn, (
                f'Only classes of "{",".join(_STAT_CLASSES.keys())}" '
                "are supported when setting mode"
            )
            for perm in perms:
                if perm in {"s", "t"}:
                    result |= _STAT_EXTRA_PERMS.get((perm, stat_cls), 0) << 9
                else:
                    mask = _STAT_PERMS.get(perm, None)
                    assert mask, (
                        f'Only permissions of "{",".join(_STAT_PERMS.keys())}" '
                        "are supported when setting mode"
                    )
                    result |= stat_cls_fn(mask)
    return f"{result:04o}"


def mode_to_str(mode: Mode) -> str:
    if isinstance(mode, int):
        return f"{mode:04o}"
    # The symbolic mode must be applied after 0ing all bits.
    return f"a-rwxXst,{mode}"


# Future: this should validate that the user & group actually exist in the
# image's passwd/group databases (blocked on having those be first-class
# objects in the image build process).
def build_stat_options(
    item,
    subvol: Subvol,
    full_target_path: str,
    *,
    do_not_set_mode=False,
    build_appliance=None,
):
    assert full_target_path.startswith(
        # pyre-fixme[6]: Expected `Union[str, typing.Tuple[str, ...]]` for 1st
        #  param but got `Path`.
        subvol.path()
    ), "{self}: A symlink to {full_target_path} would point outside the image"

    if build_appliance:
        work_dir = generate_work_dir()

        # Fall back to using the host passwd if `subvol` doesn't have the DBs.
        etc_passwd = subvol.path("/etc/passwd")
        etc_group = subvol.path("/etc/group")
        if not (etc_passwd.exists() and etc_group.exists()):
            etc_passwd = "/etc/passwd"
            etc_group = "/etc/group"

        def run(cmd, **kwargs):
            run_nspawn(
                new_nspawn_opts(
                    cmd=cmd,
                    layer=build_appliance,
                    bindmount_rw=[(subvol.path(), work_dir)],
                    bindmount_ro=[
                        (etc_passwd, "/etc/passwd"),
                        (etc_group, "/etc/group"),
                    ],
                    user=pwd.getpwnam("root"),
                    **kwargs,
                ),
                PopenArgs(),
            )

        target_path_for_run = work_dir / os.path.relpath(
            full_target_path,
            # pyre-fixme[6]: For 2nd param expected `Union[None, PathLike[str],
            #  str]` but got `Path`.
            subvol.path(),
        )
    else:
        run = subvol.run_as_root
        target_path_for_run = full_target_path

    # `chmod` lacks a --no-dereference flag to protect us from following
    # `full_target_path` if it's a symlink.  As far as I know, this should
    # never occur, so just let the exception fly.
    run(["test", "!", "-L", target_path_for_run])

    if do_not_set_mode:
        assert getattr(item, "mode", None) is None, item
    else:
        # -R is not a problem since it cannot be the case that we are
        # creating a directory that already has something inside it.  On the
        # plus side, it helps with nested directory creation.
        run(
            [
                "chmod",
                "--recursive",
                mode_to_str(item.mode),
                target_path_for_run,
            ]
        )

    run(
        [
            "chown",
            "--no-dereference",
            "--recursive",
            item.user_group,
            target_path_for_run,
        ],
    )
