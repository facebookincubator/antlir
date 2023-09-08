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
        parent_depgraph: Artifact | None,
        features_json: typing.Any,
        format: str,
        subvol: Artifact | None,
        dependency_layers: list[LayerInfo],
        identifier_prefix: str = "") -> Artifact:
    output = ctx.actions.declare_output(identifier_prefix + "depgraph." + format + (".pre" if not subvol else ""))
    ctx.actions.run(
        cmd_args(
            # Inspecting already-built images often requires root privileges
            "sudo" if subvol else cmd_args(),
            ctx.attrs.antlir2[RunInfo],
            "depgraph",
            cmd_args(str(ctx.label), format = "--label={}"),
            format,
            cmd_args(features_json, format = "--feature-json={}") if features_json else cmd_args(),
            cmd_args(parent_depgraph, format = "--parent={}") if parent_depgraph else cmd_args(),
            cmd_args([li.depgraph for li in dependency_layers], format = "--image-dependency={}"),
            cmd_args(subvol, format = "--add-built-items={}") if subvol else cmd_args(),
            cmd_args(output.as_output(), format = "--out={}"),
        ),
        category = "antlir2_depgraph",
        identifier = identifier_prefix + format + ("/pre" if not subvol else ""),
        local_only = bool(subvol),
        env = {
            "RUST_LOG": "antlir2=trace",
        },
    )
    return output
