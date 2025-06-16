# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:feature.bzl", "shared_features_attrs")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg")
load("//antlir/antlir2/bzl/image:layer.bzl", "layer_rule")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo", "FeaturePluginPluginKind")

def _impl(ctx: AnalysisContext) -> Promise:
    return ctx.actions.anon_target(layer_rule, {
        "antlir2": ctx.attrs._antlir2,
        "build_appliance": ctx.attrs.build_appliance,
        "flavor": ctx.attrs.flavor,
        "name": str(ctx.label.raw_target()),
        "parent_layer": ctx.attrs.layer,
        "_analyze_feature": ctx.attrs._analyze_feature,
        "_feature_features": [ctx.attrs._dot_meta_feature],
        "_plugins": ctx.attrs._plugins,
        "_rootless": ctx.attrs._rootless,
        "_run_container": ctx.attrs._run_container,
        "_selected_target_arch": ctx.attrs._target_arch,
        "_working_format": ctx.attrs._working_format,
    }).promise.map(lambda l: [l[LayerInfo], l[DefaultInfo]])

stamp_buildinfo_rule = rule(
    impl = _impl,
    attrs = {
                "layer": attrs.dep(providers = [LayerInfo]),
                "_analyze_feature": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_depgraph_if:analyze"),
                "_antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
                "_dot_meta_feature": attrs.dep(default = "antlir//antlir/antlir2/bzl/package:dot-meta", pulls_plugins = [FeaturePluginPluginKind]),
                "_plugins": attrs.list(
                    attrs.dep(providers = [FeaturePluginInfo]),
                    default = [],
                    doc = "Used as a way to pass plugins to anon layer targets",
                ),
                "_run_container": attrs.exec_dep(default = "antlir//antlir/antlir2/container_subtarget:run"),
                "_target_arch": attrs.default_only(attrs.string(
                    default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
                )),
            } |
            {
                "_feature_" + key: val
                for key, val in shared_features_attrs.items()
            } | attrs_selected_by_cfg(),
    doc = """
    Stamp build info into a layer that is about to be packaged up.
    """,
    # Because this can instantiate an implicit layer, it must also
    # depend on the feature plugins
    uses_plugins = [FeaturePluginPluginKind],
)

stamp_buildinfo = rule_with_default_target_platform(stamp_buildinfo_rule)
