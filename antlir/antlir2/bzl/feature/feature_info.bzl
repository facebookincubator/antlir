# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:utils.bzl", "map_val")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")

# A dependency of a feature that is not yet resolved.
# This is of very limited use at parse time, but allows the feature macro to
# inform the rule what strings are actually dependencies that need to be
# resolved.
ParseTimeDependency = [
    str,
    Select,
    # this gross combo is for path_actions_t @oss-disable
    # @oss-disable
]

ParseTimeFeature = record(
    feature_type = str,
    # Plugin that implements this feature
    plugin = str,
    # Items in this list may be either raw source files, or dependencies
    # produced by another rule. If a dependency, the full provider set will be
    # made available to the analysis code for the feature.
    deps_or_srcs = field([dict[str, ParseTimeDependency], None], default = None),
    # Items in this list must be coerce-able to an "artifact"
    srcs = field([dict[str, str | Select], None], default = None),
    # These items must be `deps` and will be validated early in analysis time to
    # contain the required providers
    deps = field([dict[str, ParseTimeDependency], None], default = None),
    # Deps resolved for the execution platform. These should not be installed
    # into images because they are produced only to be run on the build worker
    exec_deps = field([dict[str, ParseTimeDependency], None], default = None),
    # Sources/deps that do not require named tracking between the parse and
    # analysis phases. Useful to support `select` in features that accept lists
    # of dependencies.
    unnamed_deps_or_srcs = field([list[ParseTimeDependency], None], default = None),
    # attrs.arg values
    args = field(dict[str, str | Select] | Select | None, default = None),
    # Plain data that defines this feature, aside from input artifacts/dependencies
    kwargs = dict[str, typing.Any],
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    build_phase = field(BuildPhase, default = BuildPhase("compile")),
    # Set `target_compatible_with` on the feature / layer target
    target_compatible_with = field(list[str] | None, default = None),
)

# Produced by the feature implementation, this tells the rule how to build it
FeatureAnalysis = provider(fields = {
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    "build_phase": provider_field(BuildPhase, default = BuildPhase("compile")),
    # Arbitrary feature record type (the antlir2 compiler must be able to
    # deserialize this)
    "data": provider_field(typing.Any),
    "feature_type": provider_field(str),
    # Binary plugin implementation of this feature
    "plugin": provider_field(FeaturePluginInfo | Provider),
    # Artifacts that are needed to build this feature. Antlir does not
    # automatically attach any dependencies to features based on the input,
    # feature implementations must always specify it exactly (this prevents
    # building things unnecessarily)
    "required_artifacts": provider_field(list[Artifact], default = []),
    # Other image layers that are required to build this feature.
    "required_layers": provider_field(list["LayerInfo"], default = []),
    # Runnable binaries required to build this feature.
    "required_run_infos": provider_field(list[RunInfo], default = []),
    # This feature requires running 'antlir2' binaries to inform buck of dynamic
    # dependencies. If no feature requires planning, the entire step can be
    # skipped and save a few seconds of build time
    "requires_planning": provider_field(bool, default = False),
})

MultiFeatureAnalysis = provider(fields = {
    "features": provider_field(list[FeatureAnalysis]),
})

def data_only_feature_rule(
        feature_attrs: dict[str, typing.Any],
        feature_type: str,
        build_phase: BuildPhase = BuildPhase("compile")):
    default_build_phase = build_phase

    def _impl(ctx: AnalysisContext) -> list[Provider]:
        attrs = {
            key: getattr(ctx.attrs, key)
            for key in feature_attrs
        }
        build_phase = map_val(BuildPhase, attrs.pop("build_phase", None)) or default_build_phase

        return [
            DefaultInfo(),
            FeatureAnalysis(
                feature_type = feature_type,
                data = struct(**attrs),
                build_phase = build_phase,
                plugin = ctx.attrs.plugin[FeaturePluginInfo],
            ),
        ]

    return rule(
        impl = _impl,
        attrs = feature_attrs | {"plugin": attrs.exec_dep(providers = [FeaturePluginInfo])},
    )

def with_phase_override(
        feature: FeatureAnalysis,
        *,
        phase: BuildPhase) -> FeatureAnalysis:
    kwargs = {k: getattr(feature, k) for k in dir(feature)}
    kwargs["build_phase"] = phase
    kwargs.pop("to_json", None)
    return FeatureAnalysis(**kwargs)
