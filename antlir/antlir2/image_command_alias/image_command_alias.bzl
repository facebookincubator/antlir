# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:layer.bzl", "layer_rule")

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    _anon_layer = ctx.actions.anon_target(layer_rule, {
        "antlir2": ctx.attrs._layer_antlir2,
        "flavor": ctx.attrs.flavor,
        "parent_layer": ctx.attrs.layer,
        "rootless": ctx.attrs._rootless,
        "target_arch": ctx.attrs._target_arch,
        "_analyze_feature": ctx.attrs._layer_analyze_feature,
        "_feature_features": [ctx.attrs._prep_feature],
        "_new_facts_db": ctx.attrs._new_facts_db,
        "_rootless": ctx.attrs._rootless,
        "_run_container": None,
        "_selected_target_arch": ctx.attrs._target_arch,
    })

    def _with_anon_layer(layer: ProviderCollection) -> list[Provider]:
        run_info = cmd_args(
            ctx.attrs._command_alias[RunInfo],
            cmd_args(layer[LayerInfo].subvol_symlink, format = "--layer={}"),
            "--",
            ctx.attrs.exe,
            cmd_args(ctx.attrs.args),
        )
        return [
            DefaultInfo(),
            RunInfo(args = run_info),
        ]

    return _anon_layer.promise.map(_with_anon_layer)

_image_command_alias = rule(
    impl = _impl,
    attrs = {
        "args": attrs.list(attrs.string(), default = []),
        "exe": attrs.arg(),
        "layer": attrs.dep(providers = [LayerInfo]),
        "_command_alias": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/image_command_alias:command_alias")),
        "_layer_analyze_feature": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_depgraph_if:analyze"),
        "_layer_antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
        "_new_facts_db": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_facts:new-facts-db"),
        "_prep_feature": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/image_command_alias:prep")),
        "_target_arch": attrs.default_only(attrs.string(
            default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
        )),
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = layer_cfg,
)

image_command_alias = rule_with_default_target_platform(_image_command_alias)
