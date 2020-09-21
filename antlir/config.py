# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.repo_config_t import repo_config_t


def load_repo_config():
    return repo_config_t.read_resource(__package__, "config.json")
