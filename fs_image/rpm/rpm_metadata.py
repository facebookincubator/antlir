#!/usr/bin/env python3

import os
import re
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


# This comprises a pure python implementation of rpm version comparison. The
# purpose for this is so that the fs_image library does not have a dependency
# on a C library that is (for the most part) only distributed as part of rpm
# based distros. Depending on a C library complicates dependency management
# significantly in the OSS space due to the complexity of handling 3rd party C
# libraries with buck. Having this pure python implementation also eases future
# rpm usage/handling for both non-rpm based distros and different arch types.
#
# This implementation is adapted from both this blog post:
#  https://blog.jasonantman.com/2014/07/how-yum-and-rpm-compare-versions/
# and this Apache 2 licenses implementation:
#   https://github.com/sassoftware/python-rpm-vercmp/blob/master/rpm_vercmp/vercmp.py
#
# There are extensive test cases in the `test_rpm_metadata.py` test case that
# cover a wide variety of normal and weird version comparsions.
def compare_rpm_versions(a: RpmMetadata, b: RpmMetadata) -> int:
    """
        Returns:
            1 if the version of a is newer than b
            0 if the versions match
            -1 if the version of a is older than b
    """

    # This is not a rule, but it makes sense that our libs don't want to
    # compare versions of different RPMs
    if a.name != b.name:
        raise ValueError(f"Cannot compare RPM versions when names do not match")

    # First compare the epoch, if set.  If the epoch's are not the same, then
    # the higher one wins no matter what the rest of the EVR is.
    if a.epoch != b.epoch:
        if a.epoch > b.epoch:
            return 1  # a > b
        else:
            return -1  # a < b

    # Epoch is the same, if version + release are the same we have a match
    if (a.version == b.version) and (a.release == b.release):
        return 0  # a == b

    # Compare version first, if version is equal then compare release
    compare_res = _compare_values(a.version, b.version)
    if compare_res != 0:  # a > b || a < b
        return compare_res
    else:
        return _compare_values(a.release, b.release)


R_NON_ALPHA_NUM_TILDE = re.compile(r"^([^a-zA-Z0-9~]*)(.*)$")
R_NUM = re.compile(r"^([\d]+)(.*)$")
R_ALPHA = re.compile(r"^([a-zA-Z]+)(.*)$")


def _compare_values(a: str, b: str) -> int:
    if a == b:
        return 0

    while a or b:
        match_a = R_NON_ALPHA_NUM_TILDE.match(a)
        match_b = R_NON_ALPHA_NUM_TILDE.match(b)
        a_head, a = match_a.group(1), match_a.group(2)
        b_head, b = match_b.group(1), match_b.group(2)

        # Ignore anything at the start we don't want
        if a_head or b_head:
            continue

        # Look at tilde first, it takes precedent over everything else
        if a.startswith('~'):
            if not b.startswith('~'):
                return -1  # a < b

            # Strip the tilde and start again
            a, b = a[1:], b[1:]
            continue

        if b.startswith('~'):
            return 1  # a > b

        # We've run out of characters to compare.
        # Note: we have to do this after we compare the ~ madness because
        # ~'s take precedance.
        if not a or not b:
            break

        # Lets see if the values are numbers
        match_a = R_NUM.match(a)
        if match_a:
            match_b = R_NUM.match(b)
            if not match_b:  # b is not a num and nums > alphas
                return 1  # a > b
            isnum = True
        else:
            match_a = R_ALPHA.match(a)
            match_b = R_ALPHA.match(b)
            isnum = False

        # strip off the leading numeric or alpha chars
        a_head, a = match_a.group(1), match_a.group(2)
        b_head, b = match_b.group(1), match_b.group(2)

        if isnum:
            a_head = a_head.lstrip('0')
            b_head = b_head.lstrip('0')

            # Length of contiguous numbers matters
            a_head_len = len(a_head)
            b_head_len = len(b_head)
            if a_head_len < b_head_len:
                return -1  # a < b
            if a_head_len > b_head_len:
                return 1  # a > b

        # Either a number with the same number of chars or
        # the leading chars are alpha so lets do a standard compare
        if a_head < b_head:
            return -1  # a < b
        if a_head > b_head:
            return 1  # a > b

        # Both header segments are of equal length, keep going with the new
        continue  # pragma: no cover

    # Both are now zero length, that means they must be equal
    if len(a) == len(b) == 0:
        return 0  # a == b

    # Longer string is > than shorter string
    if len(a) != 0:
        return 1  # a > b

    return -1  # a < b
