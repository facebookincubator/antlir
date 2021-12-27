#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Set

from antlir.common import not_none
from antlir.config import repo_config


def render_flavor(flavor=None) -> str:
    "A Subvolume rendering of `flavor`, or `flavor_default` if None."
    flavor = flavor or repo_config().flavor_default
    return f"(File d{len(flavor)})"


def get_rpm_installers_supported() -> Set[str]:
    return {
        not_none(config.rpm_installer)
        for _, config in repo_config().flavor_to_config.items()
        if config.rpm_installer
    }
