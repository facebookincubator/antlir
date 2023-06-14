# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:toolchain.bzl", "Antlir2ToolchainInfo")

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
    # Sources/deps that do not require named tracking between the parse and
    # analysis phases. Useful to support `select` in features that accept lists
    # of dependencies.
    unnamed_deps_or_sources = field([[[str.type, "selector"]], None], default = None),
    # Plain data that defines this feature, aside from input artifacts/dependencies
    kwargs = {str.type: ""},
    analyze_uses_context = field(bool.type, default = False),
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    build_phase = field(BuildPhase.type, default = BuildPhase("compile")),
)

# Produced by the feature implementation, this tells the rule how to build it
FeatureAnalysis = record(
    feature_type = str.type,
    # Arbitrary feature record type (the antlir2 compiler must be able to
    # deserialize this)
    data = "record",
    # Artifacts that are needed to build this feature. Antlir does not
    # automatically attach any dependencies to features based on the input,
    # feature implementations must always specify it exactly (this prevents
    # building things unnecessarily)
    required_artifacts = field(["artifact"], default = []),
    # Runnable binaries required to build this feature.
    required_run_infos = field(["RunInfo"], default = []),
    # Other image layers that are required to build this feature.
    required_layers = field(["LayerInfo"], default = []),
    # This feature requires running 'antlir2' binaries to inform buck of dynamic
    # dependencies. If no feature requires planning, the entire step can be
    # skipped and save a few seconds of build time
    requires_planning = field(bool.type, default = False),
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    build_phase = field(BuildPhase.type, default = BuildPhase("compile")),
)

Toolchains = record(
    cxx = "dependency",
)

AnalyzeFeatureContext = record(
    toolchain = Antlir2ToolchainInfo.type,
    unique_action_identifier = str.type,
    actions = "actions",
    toolchains = Toolchains.type,
)

def data_only_feature_analysis_fn(
        record_type,
        feature_type: str.type,
        build_phase: BuildPhase.type = BuildPhase("compile")):
    # @lint-ignore BUCKRESTRICTEDSYNTAX
    def inner(**kwargs) -> FeatureAnalysis.type:
        return FeatureAnalysis(
            feature_type = feature_type,
            data = record_type(**kwargs),
            build_phase = build_phase,
        )

    return inner

def with_phase_override(
        feature: FeatureAnalysis.type,
        *,
        phase: BuildPhase.type) -> FeatureAnalysis.type:
    kwargs = {k: getattr(feature, k) for k in dir(feature)}
    kwargs["build_phase"] = phase
    return FeatureAnalysis(**kwargs)
