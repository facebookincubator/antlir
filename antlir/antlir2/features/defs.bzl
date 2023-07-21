# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "rust_binary", "rust_library")

def feature_impl(
        *,
        name: str.type,
        src: [str.type, None] = None,
        extra_srcs: [str.type] = [],
        deps: [str.type] = [],
        unstable_features: [str.type] = [],
        **kwargs):
    rust_library(
        name = name + ".lib",
        srcs = [src or name + ".rs"] + extra_srcs,
        crate_root = src or name + ".rs",
        rustc_flags = list(kwargs.pop("rustc_flags", [])) + [
            "-Zcrate-attr=feature({})".format(feat)
            for feat in unstable_features
        ],
        crate = name,
        deps = deps + [
            "anyhow",
            "serde",
            "tracing",
            "//antlir/antlir2/antlir2_compile:antlir2_compile",
            "//antlir/antlir2/antlir2_depgraph:antlir2_depgraph",
            "//antlir/antlir2/antlir2_features:antlir2_features",
            "//antlir/antlir2/features/antlir2_feature_impl:antlir2_feature_impl",
        ],
        visibility = ["//antlir/antlir2/..."],
    )
    rust_binary(
        name = name,
        mapped_srcs = {
            "//antlir/antlir2/features:main.rs": "src/main.rs",
        },
        named_deps = {
            "impl": ":{}.lib".format(name),
        },
        deps = [
            "anyhow",
            "clap",
            "serde_json",
            "tracing-glog",
            "tracing-subscriber",
            "//antlir/antlir2/antlir2_compile:antlir2_compile",
            "//antlir/antlir2/features/antlir2_feature_impl:antlir2_feature_impl",
            "//antlir/util/cli/json_arg:json_arg",
        ],
        unittests = False,
        visibility = ["PUBLIC"],
    )
