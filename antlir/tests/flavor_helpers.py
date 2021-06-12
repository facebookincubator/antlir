#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.config import load_repo_config


def render_flavor(flavor=None) -> str:
    "A Subvolume rendering of `flavor`, or `flavor_default` if None."
    flavor = flavor or load_repo_config().flavor_default
    return f"(File d{len(flavor)})"


def get_rpm_installers_supported() -> {str}:
    return {
        config.rpm_installer
        for _, config in load_repo_config().flavor_to_config.items()
        if config.rpm_installer
    }
