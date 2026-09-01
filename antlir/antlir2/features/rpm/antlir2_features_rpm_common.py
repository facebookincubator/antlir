#!/usr/libexec/platform-python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# The dnf shell interface kind of sucks for how we really want to drive it as
# part of an image build tool.
# By using the api directly, we can handle errors much more reasonably (instead
# of, to name a totally reasonable operation of dnf, silently ignoring packages
# that don't exist)

# NOTE: this must be run with system python, so cannot be a PAR file
# /usr/bin/dnf itself uses /usr/libexec/platform-python, so by using that we can
# ensure that we're using the same python that dnf itself is using

import contextlib
import glob
import os
import threading

import dnf
from dnf.module.module_base import ModuleBase


class AntlirError(Exception):
    pass


def drop_stale_bdb_regions(install_root):
    """
    Berkeley DB - the only writable rpmdb backend on EL8 - records the pid of
    every process that opens the database in shared-memory region files. Those
    processes are long gone by the time we run: some exited in the build step
    that produced the parent layer, others are the `rpmkeys` invocations this
    driver makes itself. librpm's failchk still refuses to open a database whose
    regions name a dead pid, reporting `BDB1507 Thread died in Berkeley DB
    library` / `DB_RUNRECOVERY`, or blocking on a lock the dead pid never
    released. The regions hold no persistent state and are recreated on the next
    open, so drop them before opening the database. Other backends (sqlite on
    EL9+) never create these files, so this is a no-op there.
    """
    for region in glob.glob(os.path.join(install_root, "var/lib/rpm", "__db.*")):
        # the region may disappear between the glob and the unlink
        with contextlib.suppress(FileNotFoundError):
            os.unlink(region)


class LockedOutput:
    def __init__(self, file):
        self._file = file
        self._lock = threading.Lock()

    def __enter__(self):
        self._lock.acquire()
        return self._file

    def __exit__(self, exc_type, exc_value, traceback):
        self._lock.release()


def package_struct(pkg):
    return {
        "name": pkg.name,
        "epoch": pkg.epoch,
        "version": pkg.version,
        "release": pkg.release,
        "arch": pkg.arch,
    }


def compute_explicitly_installed_package_names(spec, local_rpms):
    explicitly_installed_package_names = set()
    for item in spec["items"]:
        action = item["action"]
        rpm = item["rpm"]
        if "subject" in rpm:
            source = rpm["subject"]
        elif "src" in rpm:
            source = local_rpms[rpm["src"]]
        else:
            raise AntlirError(f"none of {{subject,src}} not found in: {rpm}")

        if action in ("install", "upgrade"):
            if isinstance(source, dnf.package.Package):
                explicitly_installed_package_names.add(source.name)
            else:
                explicitly_installed_package_names.update(
                    {
                        nevra.name
                        for nevra in dnf.subject.Subject(
                            source
                        ).get_nevra_possibilities()
                    }
                )

    return explicitly_installed_package_names


def enable_modules(items, base):
    module_base = ModuleBase(base)
    module_enable = []
    for item in items:
        if item["action"] == "module_enable":
            module_spec = item["rpm"]["subject"]
            module_base.enable([module_spec])
            module_enable.append(module_spec)
    return module_enable
