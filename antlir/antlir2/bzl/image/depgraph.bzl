# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:collections.bzl", "collections")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "LayerInfo")

def build_depgraph(ctx: "context", format: str.type, subvol: ["artifact", None], dependency_layers: ["LayerInfo"]) -> "artifact":
    output = ctx.actions.declare_output("depgraph." + format + (".pre" if not subvol else ""))
    ctx.actions.run(
        cmd_args(
            # Inspecting already-built images often requires root privileges
            "sudo" if subvol else cmd_args(),
            ctx.attrs.antlir2[RunInfo],
            "depgraph",
            cmd_args(str(ctx.label), format = "--label={}"),
            format,
            ctx.attrs.features[FeatureInfo].json_files.project_as_args("feature_json") if hasattr(ctx.attrs, "features") else cmd_args(),
            cmd_args(
                ctx.attrs.parent_layer[LayerInfo].depgraph,
                format = "--parent={}",
            ) if hasattr(ctx.attrs, "parent_layer") and ctx.attrs.parent_layer else cmd_args(),
            cmd_args(collections.uniq([li.depgraph for li in dependency_layers]), format = "--image-dependency={}"),
            cmd_args(subvol, format = "--add-built-items={}") if subvol else cmd_args(),
            cmd_args(output.as_output(), format = "--out={}"),
        ),
        category = "antlir2_depgraph",
        identifier = format + ("/pre" if not subvol else ""),
        local_only = bool(subvol) or bool(dependency_layers),
    )
    return output