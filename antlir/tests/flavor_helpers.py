#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.config import load_repo_config


def render_flavor_default() -> str:
    flavor_default = load_repo_config().flavor_default
    return f"(File d{len(flavor_default)})"
