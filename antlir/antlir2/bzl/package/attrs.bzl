# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo", "FeaturePluginPluginKind")
load(":cfg.bzl", "layer_attrs")

# Attrs that are required by all packages
common_attrs = {
    "labels": attrs.list(attrs.string(), default = []),
    "out": attrs.option(attrs.string(doc = "Output filename"), default = None),
    "_plugins": attrs.list(
        attrs.dep(providers = [FeaturePluginInfo]),
        default = [],
        doc = "Used as a way to pass plugins to anon layer targets",
    ),
} | layer_attrs

# Attrs that are not expected for users to pass
default_attrs = {
    "_analyze_feature": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_depgraph_if:analyze"),
    "_antlir2": attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2:antlir2"),
    "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
    "_dot_meta_feature": attrs.dep(default = "antlir//antlir/antlir2/bzl/package:dot-meta", pulls_plugins = [FeaturePluginPluginKind]),
    "_run_container": attrs.exec_dep(default = "antlir//antlir/antlir2/container_subtarget:run"),
    "_target_arch": attrs.default_only(attrs.string(
        default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
    )),
} | attrs_selected_by_cfg
