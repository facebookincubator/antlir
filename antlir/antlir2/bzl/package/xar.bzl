# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/package:cfg.bzl", "layer_attrs")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output(ctx.label.name + ".xar")
    spec = ctx.actions.write_json(
        "spec.json",
        {"xar": {
            "executable": ctx.attrs.executable,
            "layer": ctx.attrs.layer[LayerInfo].subvol_symlink,
            "make_xar": ctx.attrs._make_xar[RunInfo],
        }},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(out.as_output(), format = "--out={}"),
            cmd_args(spec, format = "--spec={}"),
        ),
        category = "antlir2_package",
        local_only = True,  # requires local subvol
    )
    return [
        DefaultInfo(out),
        RunInfo(cmd_args(out)),
    ]

_xar = rule(
    impl = _impl,
    attrs = {
        "executable": attrs.string(doc = "Executable within the XAR root that serves as the entrypoint"),
        "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_packager:antlir2-packager")),
        "_make_xar": attrs.default_only(attrs.exec_dep(default = "//tools/xar:make_xar")),
    } | layer_attrs,
)

xar = package_macro(_xar)
