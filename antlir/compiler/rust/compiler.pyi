#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Final, List, Mapping, Optional

from antlir.buck.buck_label.buck_label_py import Label
from antlir.bzl.constants import flavor_config_t
from antlir.fs_utils import Path

class Args:
    debug: bool
    profile_dir: Final[Optional[Path]]
    subvolumes_dir: Path
    subvolume_rel_path: Path
    child_layer_target: Label
    child_feature_json: List[Path]
    parent_layer: Final[Optional[Path]]
    flavor_config: Final[Optional[flavor_config_t]]
    version_set_override: Final[Optional[Path]]
    internal_only_is_genrule_layer: bool
    targets_and_outputs: Mapping[str, Path]
    artifacts_may_require_repo: bool
    allowed_host_mount_target: List[str]
    compiler_binary: Path
    is_nested: bool

def parse_args(argv: List[str]) -> Args: ...
