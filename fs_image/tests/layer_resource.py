#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
from typing import AnyStr

from fs_image.find_built_subvol import Subvol, find_built_subvol
from fs_image.fs_utils import Path


# This needs to be kept in sync with //fs_image/bzl:layer_resource.bzl
LAYER_SLASH_ENCODE = "%2F"


def layer_resource_subvol(package: AnyStr, name: AnyStr) -> Subvol:
    "Docs on the `layer_resource` Buck macro."
    return find_built_subvol(layer_resource(package, name).decode())


def layer_resource(package: AnyStr, name: AnyStr) -> Path:
    "Like `layer_resource_subvol`, but for the `buck-out` layer artifact."
    return Path(
        importlib.resources.read_text(
            package, name.replace("/", LAYER_SLASH_ENCODE)
        )
    )
