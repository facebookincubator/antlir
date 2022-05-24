#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import subprocess
from typing import List, NamedTuple, Optional

from antlir.common import get_logger
from antlir.fs_utils import generate_work_dir, MehStr, Path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol


# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()


class RpmMetadata(NamedTuple):
    name: str
    epoch: int
    version: str
    release: str

    @classmethod
    def from_subvol(
        cls, subvol: Subvol, ba_subvol: Subvol, package_name: str
    ) -> "RpmMetadata":
        db_path_src = subvol.path("var/lib/rpm")
        if not os.path.exists(db_path_src):
            # If we didn't check for this, `bindmount_ro` would fail.
            raise ValueError(f"RPM DB path {db_path_src} does not exist")
        # Rpm query will write to and update rpm database files if it can.
        # We must use the BA here because using the host `rpm` can cause the
        # image RPM to become unreadable to the `rpm` from the BA.
        db_path_dst = generate_work_dir()
        return _repo_query(
            db_path=db_path_dst,
            package_name=package_name,
            check_output_fn=lambda cmd: run_nspawn(
                new_nspawn_opts(
                    cmd=cmd,
                    layer=ba_subvol,
                    # Read-only so that `rpm` does not modify the DB or
                    # create one when it does not already exist.
                    bindmount_ro=[(db_path_src, db_path_dst)],
                ),
                PopenArgs(stdout=subprocess.PIPE),
            )[0].stdout,
        )

    @classmethod
    def from_file(cls, package_path: Path) -> "RpmMetadata":
        if not package_path.endswith(b".rpm"):
            raise ValueError(f"RPM file {package_path} needs to end with .rpm")
        return _repo_query(
            package_path=package_path,
            check_output_fn=subprocess.check_output,
        )


def _repo_query(
    *,
    db_path: Optional[Path] = None,
    package_name: Optional[str] = None,
    package_path: Optional[Path] = None,
    # pyre-fixme[2]: Parameter must be annotated.
    check_output_fn,
) -> "RpmMetadata":
    query_args: List[MehStr] = [
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
        result = check_output_fn(query_args).decode().strip("'\"")
        log.debug(f"RPM query {query_args} returned {result}")
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            f"Error querying RPM: {query_args}, {e.stdout}, {e.stderr}"
        )

    n, e, v, r = result.split(":")
    return RpmMetadata(name=n, epoch=int(e), version=v, release=r)


# This comprises a pure python implementation of rpm version comparison. The
# purpose for this is so that the antlir library does not have a dependency
# on a C library that is (for the most part) only distributed as part of rpm
# based distros. Depending on a C library complicates dependency management
# significantly in the OSS space due to the complexity of handling 3rd party C
# libraries with buck. Having this pure python implementation also eases future
# rpm usage/handling for both non-rpm based distros and different arch types.
#
# This implementation is adapted from both this blog post:
#  https://blog.jasonantman.com/2014/07/how-yum-and-rpm-compare-versions/
# and this Apache 2 licensed implementation:
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
        raise ValueError("Cannot compare RPM versions when names do not match")

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


# pyre-fixme[5]: Global expression must be annotated.
R_NON_ALPHA_NUM_TILDE_CARET = re.compile(rb"^([^a-zA-Z0-9~\^]*)(.*)$")
# pyre-fixme[5]: Global expression must be annotated.
R_NUM = re.compile(rb"^([\d]+)(.*)$")
# pyre-fixme[5]: Global expression must be annotated.
R_ALPHA = re.compile(rb"^([a-zA-Z]+)(.*)$")


def _compare_values(left: str, right: str) -> int:
    # Rpm versions can only be ascii, anything else is just
    # ignored
    # pyre-fixme[9]: left has type `str`; used as `bytes`.
    left = left.encode("ascii", "ignore")
    # pyre-fixme[9]: right has type `str`; used as `bytes`.
    right = right.encode("ascii", "ignore")

    if left == right:
        return 0

    while left or right:
        match_left = R_NON_ALPHA_NUM_TILDE_CARET.match(left)
        match_right = R_NON_ALPHA_NUM_TILDE_CARET.match(right)
        left_head, left = match_left.group(1), match_left.group(2)
        right_head, right = match_right.group(1), match_right.group(2)

        # Ignore anything at the start we don't want
        if left_head or right_head:
            continue

        # Look at tilde first, it takes precedent over everything else
        if left.startswith(b"~"):
            if not right.startswith(b"~"):
                return -1  # left < right

            # Strip the tilde and start again
            left, right = left[1:], right[1:]
            continue

        # Tilde always means the version is less
        if right.startswith(b"~"):
            return 1  # left > right

        # Now look at the caret, which is like the tilde but pointier.
        if left.startswith(b"^"):
            # left has a caret but right has ended
            if not right:
                return 1  # left > right

            # left has a caret but right continues on
            elif not right.startswith(b"^"):
                return -1  # left < right

            # strip the ^ and start again
            left, right = left[1:], right[1:]
            continue

        # Caret means the version is less... Unless the other version
        # has ended, then do the exact opposite.
        if right.startswith(b"^"):
            return -1 if not left else 1

        # We've run out of characters to compare.
        # Note: we have to do this after we compare the ~ and ^ madness
        # because ~'s and ^'s take precedance.
        if not left or not right:
            break

        # Lets see if we've got numbers
        match_left = R_NUM.match(left)
        if match_left:
            match_right = R_NUM.match(right)
            if not match_right:  # right is not a num and nums > alphas
                return 1  # left > right
            isnum = True
        else:  # match is alpha
            match_left = R_ALPHA.match(left)
            match_right = R_ALPHA.match(right)
            if not match_right:  # right is not an alpha and nums > alphas
                return -1  # left < right
            isnum = False

        # strip off the leading numeric or alpha chars
        left_head, left = match_left.group(1), match_left.group(2)
        right_head, right = match_right.group(1), match_right.group(2)

        if isnum:
            left_head = left_head.lstrip(b"0")
            right_head = right_head.lstrip(b"0")

            # Length of contiguous numbers matters
            left_head_len = len(left_head)
            right_head_len = len(right_head)
            if left_head_len < right_head_len:
                return -1  # left < right
            if left_head_len > right_head_len:
                return 1  # left > right

        # Either a number with the same number of chars or
        # the leading chars are alpha so lets do a standard compare
        if left_head < right_head:
            return -1  # left < right
        if left_head > right_head:
            return 1  # left > right

        # Both header segments are of equal length, keep going with the new
        continue  # pragma: no cover

    # if both are now zero length they must be equal
    if len(left) == len(right) == 0:
        return 0  # left == right

    # Longer string is > than shorter string
    if len(left) != 0:
        return 1  # left > right

    return -1  # left < right
