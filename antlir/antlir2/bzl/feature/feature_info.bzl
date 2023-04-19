# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# A dependency of a feature that is not yet resolved. This is of very limited
# use at parse time, but allows the feature definition to inform the rule what
# providers must be found on the dependency before making it to feature
# analysis.  This is analogous to a hard-coded `attrs.dep(providers=["foo"])`
# but allows each feature to depend on different types of targets.
ParseTimeDependency = record(
    dep = [str.type, "selector"],
    providers = field(["provider_callable", ""], default = []),
)

ParseTimeFeature = record(
    feature_type = str.type,
    # Items in this list may be either raw source files, or dependencies
    # produced by another rule. If a dependency, the full provider set will be
    # made available to the analysis code for the feature.
    deps_or_sources = field([{str.type: [str.type, "selector"]}, None], default = None),
    # These items must be `deps` and will be validated early in analysis time to
    # contain the required providers
    deps = field([{str.type: ParseTimeDependency.type}, None], default = None),
    # Plain data that defines this feature, aside from input artifacts/dependencies
    kwargs = {str.type: ""},
)
