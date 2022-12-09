# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:partial.bzl", "partial")
load("//antlir/bzl:query.bzl", "query")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:target_helpers.bzl", "json_targets_and_outputs")

# Find the feature JSON belonging to this layer.
def layer_features_json_query(layer):
    return query.attrfilter(
        label = "type",
        value = "image_feature",
        expr = query.deps(
            expr = query.set([layer]),
            # Limit depth to 1 to get just the `__layer-feature` target.
            # All other features are at distance 2+.
            depth = 1,
        ),
    )

# Find features JSONs and fetched package targets/outputs for the transitive
# deps of `layer`.  We need this to construct the full set of features for
# the layer and its parent layers.
def layer_included_features_query(layer):
    return query.attrregexfilter(
        label = "type",
        pattern = "|".join([
            "image_layer",
            "image_feature",
            "image_layer_from_package",
            "fetched_package_with_nondeterministic_fs_metadata",
        ]),
        expr = query.deps(
            expr = query.set([layer]),
            depth = query.UNBOUNDED,
        ),
    )

def _location(target):
    return "$(location {})".format(target)

def _targets_and_outputs(query_fn, target):
    query = query_fn(target)
    json_target = json_targets_and_outputs(
        name = sha256_b64(query),
        query = query,
    )
    return "$(location {})/targets-and-outputs.json".format(json_target)

# A convenient way to access the results of the above queries in Python
# unit tests. Use the Python function `build_env_map` to deserialize.
def test_env_map(infix_to_layer):
    return {
        "antlir_test__{}__{}".format(infix, env_name): partial.call(query_fn, target)
        for infix, target in infix_to_layer
        for env_name, query_fn in [
            ("layer_feature_json", partial.make(_targets_and_outputs, layer_features_json_query)),
            ("layer_output", partial.make(_location)),
            ("target_path_pairs", partial.make(_targets_and_outputs, layer_included_features_query)),
        ]
    }
