# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":types.bzl", "LayerInfo")

def _impl(ctx: "context") -> list["provider"]:
    out = ctx.actions.declare_output(paths.basename(ctx.attrs.path), dir = ctx.attrs.dir)
    ctx.actions.run(
        cmd_args(
            "cp",
            cmd_args("--recursive") if ctx.attrs.dir else cmd_args(),
            "--reflink=auto",
            cmd_args(
                ctx.attrs.layer[LayerInfo].subvol_symlink,
                format = "{{}}/{}".format(ctx.attrs.path.lstrip("/")),
            ),
            out.as_output(),
        ),
        category = "hoist",
        local_only = True,  # local subvol
    )
    return [
        DefaultInfo(out),
    ]

_hoist = rule(
    impl = _impl,
    attrs = {
        "dir": attrs.bool(default = False),
        "layer": attrs.dep(providers = [LayerInfo]),
        "path": attrs.string(),
    },
)

hoist = rule_with_default_target_platform(_hoist)
