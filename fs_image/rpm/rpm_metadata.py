#!/usr/bin/env python3

import os
import rpm
import subprocess

from .common import Path, get_file_logger
from subvol_utils import Subvol
from typing import NamedTuple

log = get_file_logger(__file__)


class RpmMetadata(NamedTuple):
    name: str
    epoch: int
    version: str
    release: str

    @classmethod
    def from_subvol(cls, subvol: Subvol, package_name: str) -> "RpmMetadata":
        db_path = subvol.path("var/lib/rpm")

        # `rpm` always creates a DB when `--dbpath` is an arg.
        # We don't want to create one if it does not already exist so check for
        # that here.
        if not os.path.exists(db_path):
            raise ValueError(f"RPM DB path {db_path} does not exist")

        return cls._repo_query(cls, db_path, package_name, None)

    @classmethod
    def from_file(cls, package_path: Path) -> "RpmMetadata":
        if not package_path.endswith(b'.rpm'):
            raise ValueError(f"RPM file {package_path} needs to end with .rpm")

        return cls._repo_query(cls, None, None, package_path)

    def _repo_query(
        self,
        db_path: Path,
        package_name: str,
        package_path: Path,
    ) -> "RpmMetadata":
        query_args = [
            "rpm",
            "--query",
            "--queryformat",
            "'%{NAME}:%{epochnum}:%{VERSION}:%{RELEASE}'",
        ]

        if db_path and package_name and (package_path is None):
            query_args += ["--dbpath", db_path, package_name]
        elif package_path and (db_path is None and package_name is None):
            query_args += ["--package", package_path]
        else:
            raise ValueError(
                "Must pass only (--dbpath and --package_name) or --package"
            )

        try:
            result = subprocess.check_output(
                query_args,
                stderr=subprocess.PIPE,
            ).decode().strip("'\"")
        except subprocess.CalledProcessError as e:
            raise RuntimeError(f"Error querying RPM: {e.stdout}, {e.stderr}")

        n, e, v, r = result.split(":")
        return RpmMetadata(name=n, epoch=int(e), version=v, release=r)


# Returns  1 if the version of a is newer than b
# Returns  0 if the versions match
# Returns -1 if the version of a is older than b
#
# Referenced from:
# github.com/rpm-software-management/yum/blob/master/rpmUtils/miscutils.py
def compare_rpm_versions(a: RpmMetadata, b: RpmMetadata) -> int:
    # This is not a rule, but it makes sense that our libs don't want to
    # compare versions of different RPMs
    if a.name != b.name:
        raise ValueError(f"Cannot compare RPM versions when names do not match")

    return rpm.labelCompare(
        (str(a.epoch), a.version, a.release),
        (str(b.epoch), b.version, b.release)
    )
