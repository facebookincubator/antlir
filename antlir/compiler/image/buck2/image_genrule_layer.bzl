# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the docs in antlir/website/docs/genrule-layer.md"

load("//antlir/bzl:container_opts.bzl", "normalize_container_opts")
load("//antlir/bzl:genrule_layer.shape.bzl", "genrule_layer_t")
load("//antlir/bzl:image_genrule_layer.bzl", "image_genrule_layer_helper")
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_add_feature_rule",
)
load(":compile_image_features.bzl", "compile_image_features")

def image_genrule_layer(
        name,
        rule_type,
        cmd,
        user = "nobody",
        parent_layer = None,
        flavor = None,
        flavor_config_override = None,
        container_opts = None,
        bind_repo_ro = False,
        boot = False,
        **image_layer_kwargs):
    container_opts = normalize_container_opts(container_opts)
    features = [maybe_add_feature_rule(
        name = "genrule_layer",
        include_in_target_name = {"name": name},
        feature_shape = genrule_layer_t(
            cmd = cmd,
            user = user,
            container_opts = container_opts,
            bind_repo_ro = bind_repo_ro,
            boot = boot,
        ),
    )]

    image_genrule_layer_helper(
        name,
        rule_type,
        parent_layer,
        flavor,
        flavor_config_override,
        container_opts,
        features,
        compile_image_features,
        image_layer_kwargs,
    )
