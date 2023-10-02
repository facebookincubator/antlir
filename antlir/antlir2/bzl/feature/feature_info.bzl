# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")

# A dependency of a feature that is not yet resolved. This is of very limited
# use at parse time, but allows the feature definition to inform the rule what
# providers must be found on the dependency before making it to feature
# analysis.  This is analogous to a hard-coded `attrs.dep(providers=["foo"])`
# but allows each feature to depend on different types of targets.
ParseTimeDependency = record(
    dep = [
        str,
        Select,
        # @oss-disable
    ],
    # List of provider types.
    providers = field(typing.Any, default = []),
)

ParseTimeFeature = record(
    feature_type = str,
    # Plugin that implements this feature
    plugin = str,
    # Items in this list may be either raw source files, or dependencies
    # produced by another rule. If a dependency, the full provider set will be
    # made available to the analysis code for the feature.
    deps_or_srcs = field([dict[str, [str, Select]], None], default = None),
    # Items in this list must be coerce-able to an "artifact"
    srcs = field([dict[str, [str, Select]], None], default = None),
    # These items must be `deps` and will be validated early in analysis time to
    # contain the required providers
    deps = field([dict[str, ParseTimeDependency], None], default = None),
    # Deps resolved for the execution platform. These should not be installed
    # into images because they are produced only to be run on the build worker
    exec_deps = field([dict[str, ParseTimeDependency], None], default = None),
    # Sources/deps that do not require named tracking between the parse and
    # analysis phases. Useful to support `select` in features that accept lists
    # of dependencies.
    unnamed_deps_or_srcs = field([list[[str, Select]], None], default = None),
    # attrs.arg values
    args = field(dict[str, str | Select] | Select | None, default = None),
    # Plain data that defines this feature, aside from input artifacts/dependencies
    kwargs = dict[str, typing.Any],
    analyze_uses_context = field(bool, default = False),
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    build_phase = field(BuildPhase, default = BuildPhase("compile")),
)

# Produced by the feature implementation, this tells the rule how to build it
FeatureAnalysis = record(
    feature_type = str,
    # Binary plugin implementation of this feature
    plugin = FeaturePluginInfo,
    # Arbitrary feature record type (the antlir2 compiler must be able to
    # deserialize this)
    data = typing.Any,
    # Artifacts that are needed to build this feature. Antlir does not
    # automatically attach any dependencies to features based on the input,
    # feature implementations must always specify it exactly (this prevents
    # building things unnecessarily)
    required_artifacts = field(list[Artifact], default = []),
    # Runnable binaries required to build this feature.
    required_run_infos = field(list[RunInfo], default = []),
    # Other image layers that are required to build this feature.
    required_layers = field(list["LayerInfo"], default = []),
    # This feature requires running 'antlir2' binaries to inform buck of dynamic
    # dependencies. If no feature requires planning, the entire step can be
    # skipped and save a few seconds of build time
    requires_planning = field(bool, default = False),
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    build_phase = field(BuildPhase, default = BuildPhase("compile")),
)

Tools = record(
    objcopy = Dependency,
)

AnalyzeFeatureContext = record(
    label = Label,
    unique_action_identifier = str,
    actions = AnalysisActions,
    tools = Tools,
)

def data_only_feature_analysis_fn(
        record_type,
        feature_type: str,
        build_phase: BuildPhase = BuildPhase("compile")):
    def inner(plugin: FeaturePluginInfo, **kwargs) -> FeatureAnalysis:
        return FeatureAnalysis(
            feature_type = feature_type,
            data = record_type(**kwargs),
            build_phase = build_phase,
            plugin = plugin,
        )

    return inner

def with_phase_override(
        feature: FeatureAnalysis,
        *,
        phase: BuildPhase) -> FeatureAnalysis:
    kwargs = {k: getattr(feature, k) for k in dir(feature)}
    kwargs["build_phase"] = phase
    return FeatureAnalysis(**kwargs)
