#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"No externally useful functions here.  Read the `run.py` docblock instead."
import subprocess
from typing import NamedTuple

from antlir.fs_utils import Path


# Our container runtimes are required to make this the `PATH` for the user
# command in the container.  This also determines which container binaries
# get shadowed by `--shadow-path`.
#
# For now, the non-booted case implicitly uses the `systemd-nspawn` default
# `PATH`, so if that changes our test will fail.  That test failure in time
# will be an opportunity to decide whether to set our own, or follow.
DEFAULT_SEARCH_PATHS = tuple(
    Path(p)
    for p in (
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    )
)
DEFAULT_PATH_ENV = b":".join(DEFAULT_SEARCH_PATHS)


class NSpawnVersion(NamedTuple):
    major: int
    full: str


def nspawn_version() -> NSpawnVersion:
    """
    We now care about the version of nspawn we are running.  The output of
    systemd-nspawn --version looks like:

    ```
    systemd 242 (v242-2.fb1)
    +PAM +AUDIT +SELINUX +IMA ...
    ```
    So we can get the major version as the second token of the first line.
    We hope that the output of systemd-nspawn --version is stable enough
    to keep parsing it like this.
    """
    parts = subprocess.check_output(
        ["systemd-nspawn", "--version"], text=True
    ).split()
    assert parts[2].startswith("(v") and parts[2].endswith(")"), parts
    return NSpawnVersion(major=int(parts[1]), full=parts[2][2:-1])
