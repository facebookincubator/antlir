# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib
import json

from antlir.artifacts_dir import find_buck_cell_root
from antlir.repo_config_t import repo_config_t as base_repo_config_t


def load_repo_config(path_in_repo=None):
    with importlib.resources.open_text(__package__, "config.json") as r:
        data = json.load(r)

    return repo_config_t(
        repo_root=find_buck_cell_root(path_in_repo=path_in_repo), **data
    )


class repo_config_t(base_repo_config_t):
    repo_root: str
