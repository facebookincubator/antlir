#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
from typing import AnyStr

from antlir.cli import normalize_buck_path
from antlir.find_built_subvol import find_built_subvol, Subvol
from antlir.fs_utils import Path


# This needs to be kept in sync with //antlir/bzl:layer_resource.bzl
LAYER_SLASH_ENCODE = "%2F"


def layer_resource_subvol(package: AnyStr, name: AnyStr) -> Subvol:
    "Docs on the `layer_resource` Buck macro."
    return find_built_subvol(layer_resource(package, name).decode())


def layer_resource(package: AnyStr, name: AnyStr) -> Path:
    "Like `layer_resource_subvol`, but for the `buck-out` layer artifact."
    return normalize_buck_path(
        importlib.resources.read_text(
            # pyre-fixme[6]: Expected `Union[str, types.ModuleType]` for 1st
            # param but got `AnyStr`.
            package,
            # pyre-fixme[6]: Expected `bytes` for 1st param but got `str`.
            name.replace("/", LAYER_SLASH_ENCODE),
        )
    )
