# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    extension = {"sendstream.v2": ".sendstream.v2", "sendstream.zst": ".sendstream.zst"}[ctx.attrs.format]
    package = ctx.actions.declare_output("image" + extension)
    spec = ctx.actions.write_json("spec.json", {ctx.attrs.format: ctx.attrs.opts})
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2_package[RunInfo],
            cmd_args(ctx.attrs.layer[LayerInfo].subvol_symlink, format = "--layer={}"),
            cmd_args(spec, format = "--spec={}"),
            cmd_args(package.as_output(), format = "--out={}"),
        ),
        local_only = True,
        category = "antlir2_package",
    )
    return [DefaultInfo(package)]

_package = rule(
    impl = _impl,
    attrs = {
        "antlir2_package": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package:antlir2-package")),
        "format": attrs.enum(["sendstream.v2", "sendstream.zst", "cpio.gz"]),
        "layer": attrs.dep(providers = [LayerInfo]),
        "opts": attrs.dict(attrs.string(), attrs.any(), default = {}, doc = "options for this package format"),
    },
)

def _new_package(
        name: str.type,
        layer: str.type,
        format: str.type,
        compression_level: int.type = 3,
        **kwargs):
    _package(
        name = name,
        layer = layer,
        format = format,
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

package = struct(
    new = _new_package,
)
