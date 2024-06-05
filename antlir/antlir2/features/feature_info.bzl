# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//utils:arglike.bzl", "ArgLike")
load("@prelude//utils:utils.bzl", "map_val")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")

def ParseTimeFeature(
        *,
        feature_type: str,
        # Plugin that implements this feature
        plugin: str,
        # Items in this list may be either raw source files, or dependencies
        # produced by another rule. If a dependency, the full provider set will be
        # made available to the analysis code for the feature.
        deps_or_srcs: dict[str, typing.Any] | None = None,
        # Items in this list must be coerce-able to an "artifact"
        srcs: dict[str, typing.Any] | None = None,
        # These items must be `deps` and will be validated early in analysis time to
        # contain the required providers
        deps: dict[str, typing.Any] | None = None,
        # Deps resolved for the execution platform. These should not be installed
        # into images because they are produced only to be run on the build worker
        exec_deps: dict[str, typing.Any] | None = None,
        # These are `deps` that should retain the antlir2 configuration (OS, ROU,
        # etc). Use this for layer deps (or anything else where this makes sense to
        # be important) only, so that antlir2 can be used in other parts of the
        # dependency graph according to whatever a user says the default is. For
        # example, an fbpkg should be able to include the result of a `package.*`
        # target, without that being reconfigured for `os="none"` like fbpkgs are
        # normally configured.
        antlir2_configured_deps: dict[str, typing.Any] | None = None,
        # Sources/deps that do not require named tracking between the parse and
        # analysis phases. Useful to support `select` in features that accept lists
        # of dependencies.
        unnamed_deps_or_srcs: list[typing.Any] | None = None,
        # attrs.arg values
        args: dict[str, typing.Any] | None = None,
        # Plain data that defines this feature, aside from input artifacts/dependencies
        kwargs = dict[str, typing.Any]):
    return (
        feature_type,
        plugin,
        kwargs,
        deps_or_srcs or {},
        srcs or {},
        deps or {},
        exec_deps or {},
        antlir2_configured_deps or {},
        unnamed_deps_or_srcs or [],
        args or {},
    )

Planner = record(
    # the function to be called to do the plan operation
    fn = field(typing.Any),
    # Additional kwrgs to pass to 'fn'
    kwargs = field(dict[str, typing.Any], default = {}),
    # plan requires access to the layer label
    label = field(bool, default = False),
    # plan requires access to the layer's FlavorInfo
    flavor = field(bool, default = False),
    # plan requires access to the layer up to this point
    parent_layer_contents = field(bool, default = False),
    # plan requires access to dnf repos available to this layer
    dnf = field(bool, default = False),
    # plan requires access to the build appliance for this layer
    build_appliance = field(bool, default = False),
    # plan requires access to the layer's target_arch (x86_64, aarch64)
    target_arch = field(bool, default = False),
    # THAR BE DRAGONS
    # This is a hack to allow a feature planner to get access to plan results
    # from the previous phase. This exists *only* for chef_solo, and should be
    # *VERY* carefully considered before adopting in any other use case. It only
    # exists for chef so that the previously materialized repos in the
    # package-installation phase can be made available to the full chef run
    previous_phase_plans = field(list[str], default = []),
)

# Produced by the feature implementation, this tells the rule how to build it
FeatureAnalysis = provider(fields = {
    # Arbitrary data that is available during buck2 analysis but is not
    # serialized to JSON for the compiler (so artifacts referenced here will not
    # be accidentally materialized)
    "buck_only_data": provider_field(typing.Any, default = None),
    # Some features do mutations to the image filesystem that cannot be
    # discovered in the depgraph, so those features are grouped together in
    # hidden internal layer(s) that acts as the parent layer(s) for the final
    # image.
    "build_phase": provider_field(BuildPhase, default = BuildPhase("compile")),
    # Arbitrary feature record type (the antlir2 compiler must be able to
    # deserialize this)
    "data": provider_field(typing.Any),
    "feature_type": provider_field(str),
    # This feature requires running some logic within the feature's plugin
    # implementation to inform buck of dynamic dependencies.
    "planner": provider_field(Planner | None, default = None),
    # Binary plugin implementation of this feature
    "plugin": provider_field(FeaturePluginInfo | Provider),
    # Some features must be folded into one feature that acts on all the inputs
    # at once (typically package manager features) for performance, correctness
    # or both. This function will be called with two arguments, both of type
    # `feature_record` and the return result should be a single, merged
    # `feature_record` that represents the combination of both
    "reduce_fn": provider_field(typing.Any, default = None),
    # Artifacts that are needed to build this feature. Antlir does not
    # automatically attach any dependencies to features based on the input,
    # feature implementations must always specify it exactly (this prevents
    # building things unnecessarily)
    "required_artifacts": provider_field(list[Artifact], default = []),
    # Runnable binaries required to build this feature.
    "required_run_infos": provider_field(list[RunInfo], default = []),
})

MultiFeatureAnalysis = provider(fields = {
    "features": provider_field(list[FeatureAnalysis]),
})

PlanInfo = provider(fields = {
    "hidden": provider_field(ArgLike),
    # Unique string identifying this plan artifact for retrieval by the feature
    # that produced it
    "id": provider_field(str),
    "output": provider_field(Artifact),
    # Expose some artifacts as debug subtargets
    "sub_artifacts": provider_field(dict[str, Artifact], default = {}),
})

feature_record = record(
    feature_type = str,
    label = TargetLabel,
    analysis = FeatureAnalysis,
    plugin = FeaturePluginInfo | Provider,
)

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
