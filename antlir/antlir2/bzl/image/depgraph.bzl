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
        add_items_from_facts_db: Artifact | None,
        identifier_prefix: str = "") -> Artifact:
    output = ctx.actions.declare_output(identifier_prefix + "depgraph" + (".pre" if not add_items_from_facts_db else ""))
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2[RunInfo],
            "depgraph",
            cmd_args(features_json, format = "--feature-json={}") if features_json else cmd_args(),
            cmd_args(add_items_from_facts_db, format = "--add-built-items={}") if add_items_from_facts_db else cmd_args(),
            cmd_args(parent, format = "--parent={}") if parent else cmd_args(),
            cmd_args(output.as_output(), format = "--out={}"),
        ),
        category = "antlir2_depgraph",
        identifier = identifier_prefix.removesuffix("_") + ("/pre" if not add_items_from_facts_db else ""),
        env = {
            "RUST_LOG": "antlir2=trace",
        },
    )
    return output
