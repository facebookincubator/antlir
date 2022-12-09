#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Any, Dict, Mapping

from antlir.buck.targets_and_outputs.targets_and_outputs_py import TargetsAndOutputs
from antlir.cli import normalize_buck_path
from antlir.fs_utils import Path
from antlir.rpm.replay.extract_nested_features import (
    extract_nested_features,
    ExtractedFeatures,
)


def _layer_feature_json(path: Path) -> Path:
    layer_feature_json_map = list(
        TargetsAndOutputs.from_file(normalize_buck_path(path)).dict().values()
    )
    layer_feature_json = layer_feature_json_map.pop()
    assert len(layer_feature_json_map) == 0, "should have had exactly one element"
    return layer_feature_json


def build_env_map(environ: Mapping[str, str], infix: str) -> Dict[str, Any]:
    prefix = f"antlir_test__{infix}__"
    layer_output = Path(environ[prefix + "layer_output"])
    target_map = TargetsAndOutputs.from_file(
        normalize_buck_path(Path(environ[prefix + "target_path_pairs"]))
    )
    return {
        "layer_output": layer_output,
        "layer_feature_json": _layer_feature_json(
            Path(environ[prefix + "layer_feature_json"])
        ),
        "target_map": target_map,
    }


def extract_features_from_env_map(env_map: Dict[str, Any]) -> ExtractedFeatures:
    return extract_nested_features(
        layer_features_out=env_map["layer_feature_json"],
        layer_out=Path(env_map["layer_output"]),
        target_to_path=env_map["target_map"],
    )
