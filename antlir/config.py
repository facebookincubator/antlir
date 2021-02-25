# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib
import json
from typing import Optional

from antlir.artifacts_dir import find_artifacts_dir, find_repo_root
from antlir.fs_utils import Path
from antlir.repo_config_t import repo_config_t as base_repo_config_t

_read_text = importlib.resources.read_text


def load_repo_config(path_in_repo=None):
    data = json.loads(_read_text(__package__, "config.json"))

    # If we don't need the artifacts, then it's reasonable that we might
    # not find a repo root.  We can safely ignore the error and not have
    # a repo_root in that case.  But if we *do* need the artifacts, we
    # should fail loudly here because things will likely be broke.
    # In addition, if we don't have a repo_root, we can't have an
    # artifact dir either.
    repo_root = None
    host_mounts_for_repo_artifacts = data.pop(
        "host_mounts_for_repo_artifacts", []
    )
    try:
        repo_root = find_repo_root(path_in_repo=path_in_repo)
        artifact_dir = find_artifacts_dir(path_in_repo=path_in_repo)

        # If artifact_dir is a symlink then we need to include the real
        # path as a host_mount_for_repo_artifacts entry so that the image
        # build volume is included.
        if artifact_dir.islink():
            host_mounts_for_repo_artifacts.append(artifact_dir.realpath())

    except RuntimeError as re:
        if data.get("artifacts_require_repo"):
            raise re

    return repo_config_t(
        repo_root=repo_root,
        host_mounts_for_repo_artifacts=host_mounts_for_repo_artifacts,
        **data
    )


class repo_config_t(base_repo_config_t):
    repo_root: Optional[Path] = None
