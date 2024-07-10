# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":defs.bzl", "common_attrs", "default_attrs", "squashfs_anon")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output(ctx.label.name)
    squash = ctx.actions.anon_target(squashfs_anon, {
        k: getattr(ctx.attrs, k)
        for k in list(layer_attrs) + list(common_attrs) + list(default_attrs)
    }).artifact("package")
    spec = ctx.actions.write_json(
        "spec.json",
        {"xar": {
            "executable": ctx.attrs.executable,
            "squashfs": squash,
            "target_name": ctx.label.name,
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
        identifier = "xar",
        local_only = True,  # requires local subvol
    )
    return [
        DefaultInfo(out, sub_targets = {"squashfs": [DefaultInfo(squash)]}),
        RunInfo(cmd_args(out)),
    ]

_xar = rule(
    impl = _impl,
    attrs = {
        "executable": attrs.string(doc = "Executable within the XAR root that serves as the entrypoint"),
    } | layer_attrs | default_attrs | common_attrs,
    cfg = package_cfg,
)

xar = package_macro(_xar)
