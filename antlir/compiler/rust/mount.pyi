#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Final, Iterator, List, Mapping, NamedTuple, Optional

from antlir.bzl.constants import flavor_config_t
from antlir.fs_utils import Path

class BuildSource(NamedTuple):
    type: str
    source: str

    def to_path(
        self, *, target_to_path: Mapping[str, Path], subvolumes_dir: Path
    ) -> Path: ...

class RuntimeSource:
    type: str
    # Note: these are specific to the FB runtime
    package: Optional[str] = None
    tag: Optional[str] = None
    uuid: Optional[str] = None

class LayerPublisher:
    package: str
    # JSON contents of a shape target which can then be parsed
    shape_target_contents: str

class Mount:
    mountpoint: str
    build_source: BuildSource
    is_directory: bool
    runtime_source: Optional[RuntimeSource] = None
    layer_publisher: Optional[LayerPublisher] = None

def mounts_from_meta(volume_path: Path) -> Iterator[Mount]: ...
