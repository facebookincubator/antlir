# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:types.bzl",
    "LayerInfo",  # @unused Used as type
)

def build_depgraph(
        *,
        ctx: AnalysisContext,
        parent: Artifact | None,
        features_json: typing.Any,
        identifier: str | None = None) -> Artifact:
    if identifier:
        output = ctx.actions.declare_output(identifier, "depgraph")
    else:
        output = ctx.actions.declare_output("depgraph")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2[RunInfo],
            "depgraph",
            cmd_args(features_json, format = "--feature-json={}") if features_json else cmd_args(),
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
