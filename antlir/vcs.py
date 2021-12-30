#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import subprocess
import sys
from datetime import datetime
from typing import Optional, Union

from antlir.artifacts_dir import find_repo_root
from antlir.fs_utils import Path


class _Hg:
    def rev_id(self, rev: Optional[str], cwd: Optional[Path]) -> str:
        return subprocess.check_output(
            ["hg", "log", "-T", "{node}", "-r", rev or "."], text=True, cwd=cwd
        ).strip()

    def rev_timestamp(
        self, rev: Optional[str], cwd: Optional[Path]
    ) -> datetime:
        return datetime.fromisoformat(
            subprocess.check_output(
                [
                    "hg",
                    "log",
                    "--template",
                    # rfc3339date matches the strict ISO8601 format that
                    # git uses, but more importantly it outputs a format
                    # that is parsable by `datetime.fromisoformat(...)`.
                    # Like this: 2021-12-29T18:22:22-08:00
                    "{date|rfc3339date}",
                    "--rev",
                    rev or ".",
                ],
                text=True,
                cwd=cwd,
            ).strip()
        )


class _Git:
    def rev_id(self, rev: Optional[str], cwd: Optional[Path]) -> str:
        return subprocess.check_output(
            ["git", "rev-parse", rev or "HEAD"], text=True, cwd=cwd
        ).strip()

    def rev_timestamp(
        self, rev: Optional[str], cwd: Optional[Path]
    ) -> datetime:
        return datetime.fromisoformat(
            subprocess.check_output(
                [
                    "git",
                    "show",
                    "--no-patch",
                    "--format=%cI",  # %cI == strict ISO 8601 format
                    rev or "HEAD",
                ],
                text=True,
                cwd=cwd,
            ).strip()
        )


def _new_vcs(path_in_repo: Optional[Path] = None) -> Union[_Hg, _Git]:
    repo_root = find_repo_root(path_in_repo=path_in_repo)
    if Path(repo_root / ".hg").exists():
        return _Hg()
    elif Path(repo_root / ".git").exists():
        return _Git()
    else:
        raise RuntimeError(
            f"No hg or git root found in any ancestor of {path_in_repo}."
        )


def rev_id(rev: Optional[str] = None, cwd: Optional[Path] = None) -> str:
    return _new_vcs(path_in_repo=cwd).rev_id(rev=rev, cwd=cwd)


def rev_timestamp(
    rev: Optional[str] = None,
    cwd: Optional[Path] = None,
) -> datetime:
    return _new_vcs(path_in_repo=cwd).rev_timestamp(rev=rev, cwd=cwd)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--rev",
        action="store_true",
        help="Print the current revision id",
    )
    parser.add_argument(
        "--timestamp",
        action="store_true",
        help="Print the current revision timestamp in ISO8601 format.",
    )
    opts = parser.parse_args(sys.argv[1:])

    if opts.rev:
        print(rev_id())
    if opts.timestamp:
        print(rev_timestamp().strftime("%Y-%m-%dT%H:%M:%S"))
