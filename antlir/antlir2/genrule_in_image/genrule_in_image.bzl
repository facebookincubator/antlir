# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:layer.bzl", "layer_rule")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    out = None
    out_is_dir = False
    default_info = None

    if ctx.attrs.out and ctx.attrs.outs:
        fail("out and outs cannot be specified together")
    elif ctx.attrs.out:
        if ctx.attrs.out == "." or ctx.attrs.out.endswith("/"):
            out_is_dir = True
            out = ctx.actions.declare_output("out", dir = True)
        else:
            out = ctx.actions.declare_output("out")
        default_info = DefaultInfo(out)
        if ctx.attrs.default_out:
            fail("default_out cannot be combined with out")
    elif ctx.attrs.outs:
        out_is_dir = True
        out = ctx.actions.declare_output("out", dir = True)
        default_out = out
        if ctx.attrs.default_out:
            default_out = out.project(ctx.attrs.default_out)
        default_info = DefaultInfo(default_out, sub_targets = {
            name: [DefaultInfo(out.project(path))]
            for name, path in ctx.attrs.outs.items()
        })
    else:
        fail("out or outs is required")

    def _with_anon_layer(layer) -> list[Provider]:
        ctx.actions.run(
            cmd_args(
                "sudo" if not ctx.attrs._rootless else cmd_args(),
                ctx.attrs._genrule_in_image[RunInfo],
                "--rootless" if ctx.attrs._rootless else cmd_args(),
                cmd_args(layer[LayerInfo].subvol_symlink, format = "--layer={}"),
                cmd_args(out.as_output(), format = "--out={}"),
                "--dir" if out_is_dir else cmd_args(),
                "--",
                ctx.attrs.bash,
            ),
            local_only = True,  # requires local subvol
            category = "antlir2_genrule",
        )
        return [
            default_info,
        ]

    return ctx.actions.anon_target(layer_rule, {
        "antlir2": ctx.attrs._layer_antlir2,
        "flavor": ctx.attrs.flavor,
        "parent_layer": ctx.attrs.layer,
        "rootless": ctx.attrs._rootless,
        "target_arch": ctx.attrs._target_arch,
        "_feature_feature_targets": [ctx.attrs._prep_feature],
        "_new_facts_db": ctx.attrs._new_facts_db,
        "_rootless": ctx.attrs._rootless,
        "_run_container": None,
        "_selected_target_arch": ctx.attrs._target_arch,
    }).promise.map(_with_anon_layer)

_genrule_in_image = rule(
    impl = _impl,
    attrs = {
        "bash": attrs.arg(),
        "default_out": attrs.option(attrs.string(), default = None),
        "layer": attrs.dep(providers = [LayerInfo]),
        "out": attrs.option(attrs.string(), default = None),
        "outs": attrs.option(attrs.dict(attrs.string(), attrs.string()), default = None),
        "_genrule_in_image": attrs.default_only(attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/genrule_in_image:genrule_in_image"))),
        "_layer_antlir2": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2:antlir2")),
        "_new_facts_db": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_facts:new-facts-db")),
        "_prep_feature": attrs.default_only(attrs.dep(default = antlir2_dep("//antlir/antlir2/genrule_in_image:prep"))),
        "_target_arch": attrs.default_only(attrs.string(
            default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        )),
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = layer_cfg,
)

_genrule_in_image_macro = rule_with_default_target_platform(_genrule_in_image)

def genrule_in_image(
        *,
        name: str,
        default_os: str | None = None,
        **kwargs):
    if should_all_images_in_package_use_default_os():
        default_os = default_os or get_default_os_for_package()
    _genrule_in_image_macro(
        name = name,
        default_os = default_os,
        **kwargs
    )
