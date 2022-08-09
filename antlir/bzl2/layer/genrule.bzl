# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the docs in antlir/website/docs/genrule-layer.md"

load("//antlir/bzl:container_opts.bzl", "normalize_container_opts")
load("//antlir/bzl:image_genrule_layer.bzl", "image_genrule_layer_helper")
load("//antlir/bzl2:compile_image_features.bzl", "compile_image_features")
load(":genrule_layer_rule.bzl", "maybe_add_genrule_layer_rule")

def layer_genrule(
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

    features = [
        # copy in buck1 version
        maybe_add_genrule_layer_rule(
            cmd = cmd,
            user = user,
            container_opts = container_opts,
            bind_repo_ro = bind_repo_ro,
            boot = boot,
            include_in_target_name = {"name": name},
        ),
    ]

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
