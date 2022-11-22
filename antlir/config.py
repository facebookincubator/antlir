# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import importlib
import json
import sys
from typing import Optional

from antlir.artifacts_dir import find_artifacts_dir, find_repo_root, SigilNotFound
from antlir.bzl.buck_isolation.buck_isolation import is_buck_using_isolation
from antlir.errors import UserError
from antlir.fs_utils import Path
from antlir.repo_config_t import (
    data as repo_config_data,
    repo_config_t as base_repo_config_t,
)


class ConfigNotFound(UserError):
    def __init__(self):
        super().__init__("repo config not found")


_read_text = importlib.resources.read_text


class repo_config_t(base_repo_config_t):
    repo_root: Optional[Path] = None
    vcs_revision: Optional[str] = None
    revision_timestamp: Optional[str] = None
    revision_time_iso8601: Optional[str] = None


# Separated for tests, which mock and thus don't want memoization.
def _unmemoized_repo_config(*, path_in_repo=None) -> repo_config_t:
    data = repo_config_data.dict()

    # If we don't need the artifacts, then it's reasonable that we might
    # not find a repo root.  We can safely ignore the error and not have
    # a repo_root in that case.  But if we *do* need the artifacts, we
    # should fail loudly here because things will likely be broke.
    # In addition, if we don't have a repo_root, we can't have an
    # artifact dir either.
    repo_root = None
    host_mounts_for_repo_artifacts = list(
        data.pop("host_mounts_for_repo_artifacts", [])
    )
    try:
        repo_root = find_repo_root(path_in_repo=path_in_repo)
        artifact_dir = find_artifacts_dir(path_in_repo=path_in_repo)

        # If artifact_dir is a symlink then we need to include the real
        # path as a host_mount_for_repo_artifacts entry so that the image
        # build volume is included.
        if artifact_dir.islink():
            host_mounts_for_repo_artifacts.append(artifact_dir.realpath())
    except SigilNotFound:
        if data.get("artifacts_require_repo") or is_buck_using_isolation():
            raise ConfigNotFound()

    return repo_config_t(
        repo_root=repo_root,
        host_mounts_for_repo_artifacts=host_mounts_for_repo_artifacts,
        vcs_revision=_read_text(__package__, "vcs_revision").strip(),
        revision_timestamp=_read_text(__package__, "revision_timestamp").strip(),
        revision_time_iso8601=_read_text(__package__, "revision_time_iso8601").strip(),
        **data,
    )


# Memoize so that most callers can just use `repo_config().field`
repo_config = functools.lru_cache(maxsize=None)(_unmemoized_repo_config)


# Keep in sync with `antlir_dep` in `bzl/target_helpers.bzl`.
# And see the doc block there for a discussion of this
def antlir_dep(target: str) -> str:
    if "//" in target or target.startswith("/"):
        raise RuntimeError(
            "Antlir deps should be expressed as a target relative to the "
            "root Antlir directory, e.g. instead of `<cell>//antlir/foo:bar` "
            "the dep should be expressed as `foo:bar`."
        )

    if target.startswith(":"):
        return f"{repo_config().antlir_cell_name}//antlir{target}"

    return f"{repo_config().antlir_cell_name}//antlir/{target}"


if __name__ == "__main__":  # pragma: no cover
    json.dump(json.loads(repo_config().json()), sys.stdout, indent=2)
    print()
