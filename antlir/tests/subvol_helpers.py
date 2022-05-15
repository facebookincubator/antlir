#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import os
import subprocess
from typing import Optional

from antlir.btrfs_diff.tests.render_subvols import (
    render_sendstream,
    RenderedTree,
)
from antlir.config import repo_config
from antlir.fs_utils import Path
from antlir.tests.flavor_helpers import render_flavor

from ..subvol_utils import Subvol


# The easiest way to render a subvolume in a test.
def render_subvol(subvol: Subvol) -> RenderedTree:
    # Determine the original ro/rw state of the subvol so we can put it back
    # the way it was after rendering.
    was_readonly = (
        subvol.run_as_root(
            ["btrfs", "property", "get", "-ts", subvol.path(), "ro"],
            text=True,
            stdout=subprocess.PIPE,
        ).stdout.strip()
        == "ro=true"
    )
    try:
        return render_sendstream(subvol.mark_readonly_and_get_sendstream())
    finally:
        subvol.set_readonly(was_readonly)


def pop_path(render, path, *default):
    assert not isinstance(path, bytes), path  # Renderings are `str`
    parts = path.lstrip("/").split("/")
    for part in parts[:-1]:
        render = render[1][part]
    return render[1].pop(parts[-1], *default)


# Future: this isn't really the right place for it, but for now we just have
# 2 places that need it, and it's annoying to create a whole new module just
# for this helper.
def check_common_rpm_render(
    test,
    rendered_subvol,
    yum_dnf: str,
    subvol: Subvol,
    *,
    no_meta: bool = False,
    is_makecache: bool = False,
) -> None:
    r = copy.deepcopy(rendered_subvol)

    # Ignore a bunch of yum / dnf / rpm spam

    if yum_dnf == "yum":
        (ino,) = pop_path(r, "var/log/yum.log")
        test.assertRegex(
            ino,
            r"^\(File m600\)$" if is_makecache else r"^\(File m600 d[0-9]+\)$",
        )
        ino, _ = pop_path(r, "var/lib/yum")
        test.assertEqual("(Dir)", ino)
    elif yum_dnf == "dnf":
        if not is_makecache:
            test.assertEqual(
                ["(Dir)", {"dnf": ["(Dir)", {"modules.d": ["(Dir)", {}]}]}],
                pop_path(r, "etc"),
            )
            ino, _ = pop_path(r, "var/lib/dnf")
            test.assertEqual("(Dir)", ino)
        for logname in [
            "dnf.log",
            "dnf.librepo.log",
            "dnf.rpm.log",
            "hawkey.log",
        ]:
            (ino,) = pop_path(r, f"var/log/{logname}")
            test.assertRegex(ino, r"^\(File d[0-9]+\)$", logname)
    else:
        raise AssertionError(yum_dnf)

    ino, _ = pop_path(r, "var/lib/rpm")
    test.assertEqual("(Dir)", ino)

    # We permit RPM installs without `/.meta` in the root.
    meta_render = {
        ".meta": [
            "(Dir)",
            {
                "build": render_meta_build_contents(subvol),
                "flavor": [render_flavor("antlir_test")],
                "private": [
                    "(Dir)",
                    {
                        "opts": [
                            "(Dir)",
                            {"artifacts_may_require_repo": ["(File d2)"]},
                        ]
                    },
                ],
            },
        ]
    }
    test.assertEqual(
        [
            "(Dir)",
            {
                "dev": ["(Dir)", {}],
                **({} if no_meta else meta_render),
                "var": ["(Dir)", {"lib": ["(Dir)", {}], "log": ["(Dir)", {}]}],
            },
        ],
        r,
    )


def get_meta_dir_contents(subvol: Optional[Subvol] = None):
    ret = [
        "(Dir)",
        {
            "flavor": [render_flavor("antlir_test")],
            "private": [
                "(Dir)",
                {
                    "opts": [
                        "(Dir)",
                        {"artifacts_may_require_repo": ["(File d2)"]},
                    ]
                },
            ],
        },
    ]

    if subvol and os.path.isdir(subvol.path("/.meta/build")):
        # pyre-fixme[16]: Undefined attribute `__setitem__`
        ret[1]["build"] = render_meta_build_contents(subvol)

    return ret


def render_meta_build_contents(subvol: Subvol):
    # Target varies for each python_unittest.
    if not os.path.isdir(subvol.path("/.meta/build")):
        return []

    target = subvol.read_path_text(Path("/.meta/build/target"))
    return [
        "(Dir)",
        {
            "revision": [f"(File d{len(repo_config().vcs_revision) + 1})"],
            "target": [f"(File d{len(target)})"],
        },
    ]
