#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Any, Dict, Mapping

from antlir.fs_utils import Path
from antlir.serialize_targets_and_outputs import make_target_path_map

from ..extract_nested_features import extract_nested_features, ExtractedFeatures


def build_env_map(environ: Mapping[str, str], infix: str) -> Dict[str, Any]:
    prefix = f"antlir_test__{infix}__"
    layer_output = Path(environ[prefix + "layer_output"])
    _, layer_feature_json = environ[prefix + "layer_feature_json"].split()
    target_map = make_target_path_map(
        environ[prefix + "target_path_pairs"].split()
    )
    return {
        "layer_output": layer_output,
        "layer_feature_json": layer_feature_json,
        "target_map": target_map,
    }


def extract_features_from_env_map(env_map: Dict[str, Any]) -> ExtractedFeatures:
    return extract_nested_features(
        layer_features_out=env_map["layer_feature_json"],
        layer_out=Path(env_map["layer_output"]),
        target_to_path=env_map["target_map"],
    )
