# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Any, AnyStr, Dict, Mapping, Optional, Sized

from antlir.fs_utils import Path

class TargetsAndOutputs(Mapping[AnyStr, Path], Sized):
    @staticmethod
    def from_file(
        path: Path, path_in_repo: Optional[Path] = None
    ) -> TargetsAndOutputs: ...
    @staticmethod
    def from_argparse(
        path: AnyStr, path_in_repo: Optional[Path] = None
    ) -> TargetsAndOutputs: ...
    @staticmethod
    def from_json_str(
        json_str: str, path_in_repo: Optional[Path] = None
    ) -> TargetsAndOutputs: ...
    # pyre-fixme[14]: `get` overrides method defined in `Mapping` inconsistently.
    def get(self, label: AnyStr) -> Optional[Path]: ...
    def dict(self) -> Dict[str, Path]: ...
