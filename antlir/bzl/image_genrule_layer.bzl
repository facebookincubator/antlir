# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the docs in antlir/website/docs/genrule-layer.md"

load("@prelude//utils:utils.bzl", "value_or")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/antlir2/bzl/image:defs.bzl?v2_only", antlir2_image = "image")
load("//antlir/bzl:build_defs.bzl", "is_buck2")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":compile_image_features.bzl", "compile_image_features")
load(":container_opts.bzl", "normalize_container_opts")
load(":flavor_impl.bzl", "flavor_to_struct")
load(":genrule_layer.shape.bzl", "genrule_layer_t")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":shape.bzl", "shape")
load(":target_helpers.bzl", "normalize_target")
load(":target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

def image_genrule_layer_helper(
        name,
        rule_type,
        parent_layer,
        flavor,
        flavor_config_override,
        container_opts,
        features,
        compile_image_features_fn,
        image_layer_kwargs,
        extra_deps = None):
    flavor = flavor_to_struct(flavor)
    if container_opts.internal_only_logs_tmpfs:
        # The mountpoint directory would leak into the built images, and it
        # doesn't even make sense for genrule layer construction.
        fail("Genrule layers do not allow setting up a `/logs` tmpfs")

    # This is not strictly needed since `image_layer_impl` lacks this kwarg.
    if "features" in image_layer_kwargs:
        fail("\"features\" are not supported in image.genrule_layer")

    # Build a new layer. It may be empty.
    _make_subvol_cmd, _deps_query = compile_image_features_fn(
        name = name,
        current_target = normalize_target(":" + name),
        parent_layer = parent_layer,
        features = features,
        flavor = flavor,
        flavor_config_override = flavor_config_override,
        internal_only_is_genrule_layer = True,
        extra_deps = extra_deps,
    )
    image_layer_utils.image_layer_impl(
        _rule_type = "image_layer_genrule_" + rule_type,
        _layer_name = name,
        _make_subvol_cmd = _make_subvol_cmd,
        _deps_query = _deps_query,
        **image_layer_kwargs
    )

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
        antlir2_mount_platform = None,
        **image_layer_kwargs):
    """
### Danger! Danger! Danger!

The resulting layer captures the result of executing a command inside
another `image.layer`.  This is a power tool intended for extending Antlir
with new macros.  It must be used with *extreme caution*.  Please
**carefully read [the full docs](/docs/genrule-layer)** before using.

Mandatory arguments:
    - `cmd`: The command to execute inside the layer.  See the [full
        docs](/docs/genrule-layer) for details on the constraints.  **PLEASE
        KEEP THIS DETERMINISTIC.**
    - `rule_type`: The resulting Buck target node will have type
        `image_layer_genrule_{rule_type}`, which allows `buck query`ing for this
        specific kind of genrule layer.  Required because the intended usage for
        genrule layers is the creation of new macros, and type-tagging lets
        Antlir maintainers survey this ecosystem without resorting to `grep`.

Optional arguments:
    - `user` (defaults to `nobody`): Run `cmd` as this user inside the image.
    - `parent_layer`: The name of another layer target, inside of which
        `cmd` will be executed.
    - `flavor`: The build flavor that will be used to load the config from
        REPO_CFG.flavor_to_config
    - `flavor_config_overrde`: A struct that contains fields that override
        the default values specific by `flavor`.
    - `container_opts`: An `image.opts` containing keys from `container_opts_t`.
        If you want to install packages, you will usually want to set
        `shadow_proxied_binaries` here.
    - `bind_repo_ro`: Bind the repository into the layer for use.  This is
        generally not advised as it creates the possibility of subverting the
        buck dependency graph and generally wreaking havok.  Use with extreme
        caution.
    - `boot`: Run `cmd` in a container booted with systemd. This should generally
        not be required except in cases where a `cmd` has assumptions about running
        in an environment where a running systemd is available.
    - See the `_image_layer_impl` signature (in `image_layer_utils.bzl`)
        for supported, but less commonly used, kwargs.
    """
    antlir2 = image_layer_kwargs.pop("antlir2", None)
    antlir2_mount_platform = value_or(antlir2_mount_platform, REPO_CFG.artifacts_require_repo)
    if antlir2_shim.upgrade_or_shadow_layer(
        antlir2 = antlir2,
        name = name,
        fn = antlir2_shim.getattr_buck2(antlir2_image, "layer"),
        parent_layer = parent_layer + ".antlir2" if parent_layer else None,
        flavor = flavor,
        features = [
            antlir2_feature.genrule(
                cmd = cmd,
                user = user,
                mount_platform = antlir2_mount_platform,
                bind_repo_ro = bind_repo_ro,
                boot = boot,
            ) if is_buck2() else None,
        ],
        implicit_antlir2 = True,
        fake_buck1 = struct(
            fn = antlir2_shim.fake_buck1_layer,
            name = name,
        ),
    ) == "upgrade":
        return

    flavor = flavor_to_struct(flavor)
    container_opts = normalize_container_opts(container_opts)

    # This is not strictly needed since `image_layer_impl` lacks this kwarg.
    target_tagger = new_target_tagger()
    features = [target_tagger_to_feature(
        target_tagger,
        struct(genrule_layer = [
            shape.as_target_tagged_dict(
                target_tagger,
                genrule_layer_t(
                    cmd = cmd,
                    user = user,
                    container_opts = container_opts,
                    bind_repo_ro = bind_repo_ro,
                    boot = boot,
                ),
            ),
        ]),
        antlir2_feature = antlir2_feature.genrule(
            cmd = cmd,
            user = user,
            mount_platform = antlir2_mount_platform,
            bind_repo_ro = bind_repo_ro,
            boot = boot,
        ) if is_buck2() else None,
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
