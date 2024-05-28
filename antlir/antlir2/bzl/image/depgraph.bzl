# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:build_phase.bzl",
    "BuildPhase",  # @unused Used as type
)
load(
    "//antlir/antlir2/bzl:types.bzl",
    "LayerInfo",  # @unused Used as type
)
load(
    "//antlir/antlir2/bzl/feature:feature.bzl",
    "feature_record",  # @unused Used as type
)

def analyze_features(
        *,
        ctx: AnalysisContext,
        features: list[feature_record | typing.Any],
        identifier: str,
        phase: BuildPhase) -> list[Artifact]:
    deduped_features = []
    analyzed_features = []
    for idx, feature in enumerate(features):
        # TODO(T177933397) completely identical features really should be banned
        # from a readability perspective, but for now we'll just dedupe them
        # here, before any analysis actions
        if feature in deduped_features:
            continue

        # TODO: figure out how to make regrouped features (aka rpm) more general
        # / sane and move this analysis into the feature anon targets instead of
        # having to do it as part of the layer
        input = ctx.actions.write_json(
            ctx.actions.declare_output(
                identifier + "/features/" + phase.value,
                "{}[{}].json".format(feature.feature_type, idx),
            ),
            struct(
                # serializing feature.analysis as a whole would cause tons of
                # unnecessary inputs to be materialized, so only analysis.data
                # is included
                data = feature.analysis.data,
                feature_type = feature.feature_type,
                label = feature.label,
                plugin = feature.plugin,
            ),
            with_inputs = True,
        )
        out = ctx.actions.declare_output(
            identifier + "/features/" + phase.value,
            "{}[{}].analyzed.json".format(feature.feature_type, idx),
        )
        ctx.actions.run(
            cmd_args(
                ctx.attrs._analyze_feature[RunInfo],
                cmd_args(input, format = "--feature={}"),
                cmd_args(out.as_output(), format = "--out={}"),
            ),
            category = "antlir2_feature_analyze",
            identifier = "{}/{}[{}]".format(phase.value, feature.feature_type, idx),
        )
        analyzed_features.append(out)
        deduped_features.append(feature)
    return analyzed_features

def build_depgraph(
        *,
        ctx: AnalysisContext,
        parent: Artifact | None,
        features: list[feature_record | typing.Any],
        identifier: str,
        phase: BuildPhase) -> Artifact:
    output = ctx.actions.declare_output(identifier, "depgraph")

    analyzed_features = analyze_features(
        ctx = ctx,
        features = features,
        identifier = identifier,
        phase = phase,
    )

    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2[RunInfo],
            "depgraph",
            cmd_args(analyzed_features, format = "--feature={}"),
            cmd_args(parent, format = "--parent={}") if parent else cmd_args(),
            cmd_args(output.as_output(), format = "--out={}"),
        ),
        category = "antlir2_depgraph",
        identifier = identifier,
        env = {
            "RUST_LOG": "antlir2=trace",
        },
    )
    return output
