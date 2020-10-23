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
    full = parts[2]
    assert full.startswith("(") and full.endswith(")"), parts
    full = parts[2][1:-1].lstrip("v")
    return NSpawnVersion(major=int(parts[1]), full=full)


def find_cgroup2_mountpoint() -> Path:
    with open("/proc/self/mounts", "rb") as mounts:
        for mount in mounts.readlines():
            if mount.startswith(b"cgroup2 "):
                return Path(mount.split()[1])
    raise RuntimeError("No cgroupv2 mountpoint found")  # pragma: no cover


def parse_cgroup2_path(proc_cgroup: bytes) -> Path:
    """
    Find the cgroup2 within the input from /proc/*/cgroup
    This ignores any other controller hierarchies that do not start with 0::
    """
    cg2_prefix = b"0::"
    my_cg = None
    for line in proc_cgroup.splitlines():
        if line.startswith(cg2_prefix):
            assert my_cg is None, f"found two cgroup matches {my_cg} {line}"
            my_cg = line
    assert my_cg is not None, f"cgroup2 is required: {proc_cgroup}"
    return Path(my_cg[len(cg2_prefix) :])
