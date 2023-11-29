# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# During the migration period of getting users onto antlir2, we need to create
# parallel definitions of antlir2 features in order to have any hope of doing a
# piecewise migration. However, there are some "features" (like
# layer_from_package) that are strictly internal implementation details of
# antlir1 and have no equivalent "feature" in antlir2 (since antlir2 does not
# have any internal-only features). To get around this, here is an antlir2
# feature that allows us to record some debugging information to chase down
# straggler use cases later.

# NOTE: this feature is NOT parseable by the antlir2 compiler, so attempting to
# build any layer that contains this feature will fail, which is what we want

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def antlir1_no_equivalent(*, label: str, description: str) -> ParseTimeFeature:
    return ParseTimeFeature(
        plugin = antlir2_dep("features/antlir1_no_equivalent:antlir1_no_equivalent"),
        feature_type = "antlir1_no_equivalent",
        kwargs = {
            "description": description,
            "label": label,
        },
    )

antlir1_no_equivalent_rule = data_only_feature_rule(
    feature_attrs = {
        "description": attrs.string(),
        "label": attrs.string(),
    },
    feature_type = "antlir1_no_equivalent",
)
