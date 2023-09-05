# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @starlark-rust: allow_string_literals_in_type_expr

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:feature.bzl", "shared_features_attrs")
load("//antlir/antlir2/bzl/image:layer.bzl", "layer_rule")

def _impl(ctx: AnalysisContext) -> "promise":
    return ctx.actions.anon_target(layer_rule, {
        "antlir2": ctx.attrs._antlir2,
        "name": str(ctx.label.raw_target()),
        "parent_layer": ctx.attrs.layer,
        "target_arch": ctx.attrs._target_arch,
        "_feature_feature_targets": [ctx.attrs._dot_meta_feature],
        "_objcopy": ctx.attrs._objcopy,
        "_run_nspawn": ctx.attrs._run_nspawn,
    }).map(lambda l: [l[LayerInfo], l[DefaultInfo]])

stamp_buildinfo_rule = rule(
    impl = _impl,
    attrs = {
                "layer": attrs.dep(providers = [LayerInfo]),
                "_antlir2": attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2"),
                "_dot_meta_feature": attrs.dep(default = "//antlir/antlir2/bzl/package:dot-meta"),
                "_objcopy": attrs.default_only(attrs.exec_dep(default = "fbsource//third-party/binutils:objcopy")),
                "_run_nspawn": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/nspawn_in_subvol:nspawn")),
                "_target_arch": attrs.default_only(attrs.string(
                    default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
                )),
            } |
            {
                "_feature_" + key: val
                for key, val in shared_features_attrs.items()
            },
    doc = """
    Stamp build info into a layer that is about to be packaged up.
    """,
)

stamp_buildinfo = rule_with_default_target_platform(stamp_buildinfo_rule)
