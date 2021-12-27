# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:query.bzl", "query")

# Find the feature JSON belonging to this layer.
def layer_features_json_query(layer):
    return "$(query_targets_and_outputs '{query}')".format(
        query = query.attrfilter(
            label = "type",
            value = "image_feature",
            expr = query.deps(
                expr = query.set([layer]),
                # Limit depth to 1 to get just the `__layer-feature` target.
                # All other features are at distance 2+.
                depth = 1,
            ),
        ),
    )

# Find features JSONs and fetched package targets/outputs for the transitive
# deps of `layer`.  We need this to construct the full set of features for
# the layer and its parent layers.
def layer_included_features_query(layer):
    return "$(query_targets_and_outputs '{query}')".format(
        query = query.attrregexfilter(
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
        ),
    )

# Any "layer package builder" implementations need to tag themselves with
# this label to be included when packaging a layer for replay deployment.
ANTLIR_INTERNAL_BUILD_PKG_LABEL = "antlir_build_pkg"

# Find all package builders for any mounted packages in `layer` (and its
# parents).  We use these to package the mounts when we package the layer.
def layer_included_builders_query(layer):
    return "$(query_targets_and_outputs '{query}')".format(
        query = query.attrfilter(
            label = "labels",
            value = ANTLIR_INTERNAL_BUILD_PKG_LABEL,
            expr = query.deps(
                expr = query.set([layer]),
                depth = query.UNBOUNDED,
            ),
        ),
    )

def _location(target):
    return "$(location {})".format(target)

# A convenient way to access the results of the above queries in Python
# unit tests. Use the Python function `build_env_map` to deserialize.
def test_env_map(infix_to_layer):
    return {
        "antlir_test__{}__{}".format(infix, env_name): query_fn(target)
        for infix, target in infix_to_layer
        for env_name, query_fn in [
            ("builders", layer_included_builders_query),
            ("layer_feature_json", layer_features_json_query),
            ("layer_output", _location),
            ("target_path_pairs", layer_included_features_query),
        ]
    }
