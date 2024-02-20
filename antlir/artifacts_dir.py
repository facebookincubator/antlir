#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"DANGER: The resulting PAR will not work if copied outside of buck-out."
import os
import subprocess
import sys
from typing import Optional

# for re-export
from antlir.artifacts_dir_rs import (  # noqa: F401
    find_buck_cell_root,
    find_repo_root,
    SigilNotFound,
)

from antlir.bzl.buck_isolation.buck_isolation import is_buck_using_isolation

from antlir.errors import UserError
from antlir.fs_utils import Path


def _is_edenfs(repo_root: Path) -> bool:
    """
    The "official" way of determining if a repository is using edenfs is to
    look for a `.eden` dir at the root of the repo.  Additionally the
    `.eden/root` symlink should point to the repository root.
    """
    eden_dir = repo_root / ".eden"
    eden_root = eden_dir / "root"

    return eden_dir.exists() and eden_root.readlink() == repo_root


def _is_edenfs_redirection(artifacts_dir: Path) -> bool:
    return artifacts_dir.islink() and b"edenfsZredirections" in artifacts_dir.realpath()


def _make_eden_redirection(
    artifacts_dir: Path,
    repo_root: Path,
) -> None:

    if artifacts_dir.exists() and not _is_edenfs_redirection(artifacts_dir):
        raise UserError(
            f"{artifacts_dir} is not a proper Edenfs redirection.\n\n"
            "Please run `buck-image-out/clean.sh` and then remove "
            "`buck-image-out` before moving forward."
        )

    ret = subprocess.run(
        [
            "edenfsctl",
            "redirect",
            "add",
            artifacts_dir,
            "symlink",
        ],
        cwd=repo_root,
        stderr=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
    )
    # Unfortunately, edenfsctl fails with an exit code of 1 if the symlink
    # already exists. It's possible that this may race with other concurrent
    # attempts. So lets check the return code here and ignore both 0 or 1 and
    # otherwise we raise the error.
    if ret.returncode > 1:
        # Let the api raise
        print(ret.stderr, file=sys.stderr)
        ret.check_returncode()


def find_artifacts_dir(path_in_repo: Optional[Path] = None) -> Path:
    "See `find_buck_cell_root`'s docblock to understand `path_in_repo`"
    if is_buck_using_isolation():
        # Future: This could be improved:
        #   - To make Pyre type-check `python_unittest`s that take
        #     dependencies on layers (e.g. `test-extract-nested-features`)
        #     we can stub out image dependencies of such tests, T118563829.
        #   - To allow layer builds in isolation settings, we'd need to
        #     extend artifact dirs to be namespaced, e.g.
        #         repo/sub_cell/ISOLATION_PREFIX-buck-out/antlir-buck1
        #         repo/buck-out/antlir-buck2-ISOLATION_DIR/
        #     Note that Buck2 will soon disallow / characters in isolation
        #     dirs, which will enable us to deduce the isolation dir.
        raise UserError(
            "Cannot build Antlir targets with --isolation_prefix or "
            "--isolation-dir. In fbsource, the likely cause is "
            "https://fburl.com/pyre-no-antlir"
        )
    return find_buck_cell_root(path_in_repo=path_in_repo) / "buck-image-out"


def ensure_per_repo_artifacts_dir_exists(
    path_in_repo: Optional[Path] = None,
) -> Path:
    "See `find_buck_cell_root`'s docblock to understand `path_in_repo`"
    repo_root = find_repo_root(path_in_repo=path_in_repo)
    artifacts_dir = find_artifacts_dir(path_in_repo=path_in_repo)

    # On Facebook infra, the repo might be hosted on an Eden filesystem,
    # which is not intended as a backing store for a large sparse loop
    # device filesystem.  We can utitlize a feature of edenfs called
    # redirections to create a suitable path for us.
    maybe_edenfs = _is_edenfs(repo_root)
    if maybe_edenfs:
        _make_eden_redirection(
            artifacts_dir,
            repo_root,
        )
    else:
        try:
            os.mkdir(artifacts_dir)
        except FileExistsError:
            pass  # We might race with another instance

    return artifacts_dir


def main() -> None:
    print(ensure_per_repo_artifacts_dir_exists())


if __name__ == "__main__":
    main()  # pragma: no cover
