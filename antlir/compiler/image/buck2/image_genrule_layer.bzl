# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the docs in antlir/website/docs/genrule-layer.md"

load("//antlir/bzl:container_opts.bzl", "normalize_container_opts")
load("//antlir/bzl:genrule_layer.shape.bzl", "genrule_layer_t")
load("//antlir/bzl:image_genrule_layer.bzl", "image_genrule_layer_helper")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
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
    if container_opts.internal_only_logs_tmpfs:
        # The mountpoint directory would leak into the built images, and it
        # doesn't even make sense for genrule layer construction.
        fail("Genrule layers do not allow setting up a `/logs` tmpfs")

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

    make_subvol_cmd = compile_image_features(
        name = name,
        current_target = normalize_target(":" + name),
        parent_layer = parent_layer,
        features = features,
        flavor = flavor,
        flavor_config_override = flavor_config_override,
        internal_only_is_genrule_layer = True,
    )

    return image_genrule_layer_helper(
        name,
        rule_type,
        make_subvol_cmd,
        image_layer_kwargs,
    )
