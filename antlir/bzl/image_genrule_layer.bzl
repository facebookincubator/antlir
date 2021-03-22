# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the docs in antlir/website/docs/genrule-layer.md"

load(":compile_image_features.bzl", "compile_image_features")
load(":container_opts.bzl", "container_opts_t", "normalize_container_opts")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":image_utils.bzl", "image_utils")
load(":shape.bzl", "shape")
load(":target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

genrule_layer_t = shape.shape(
    # IMPORTANT: Be very cautious about adding keys here, specifically
    # rejecting any options that might compromise determinism / hermeticity.
    # Genrule layers effectively run arbitrary code, so we should never
    # allow access to the network, nor read-write access to files outside of
    # the layer.  If you need something from the genrule layer, build it,
    # then reach into it with `image.source`.
    cmd = shape.list(str),
    user = str,
    container_opts = container_opts_t,
)

def image_genrule_layer(
        name,
        rule_type,
        cmd,
        user = "nobody",
        parent_layer = None,
        build_opts = None,
        container_opts = None,
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
    `image_genrule_layer_{rule_type}`, which allows `buck query`ing for this
    specific kind of genrule layer.  Required because the intended usage for
    genrule layers is the creation of new macros, and type-tagging lets
    Antlir maintainers survey this ecosystem without resorting to `grep`.

Optional arguments:
  - `user` (defaults to `nobody`): Run `cmd` as this user inside the image.
  - `parent_layer`: The name of another layer target, inside of which
    `cmd` will be executed.
  - `build_opts`: An `image.opts` containing fields accepted by
    `_build_opts` from `compile_image_features.bzl`.
  - `container_opts`: An `image.opts` containing keys from `container_opts_t`.
    If you want to install packages, you will usually want to set
    `shadow_proxied_binaries` here.
  - See the `_image_layer_impl` signature (in `image_layer_utils.bzl`)
    for supported, but less commonly used, kwargs.
    """

    # This is not strictly needed since `image_layer_impl` lacks this kwarg.
    if "features" in image_layer_kwargs:
        fail("\"features\" are not supported in image.genrule_layer")

    container_opts = normalize_container_opts(container_opts)
    if container_opts.internal_only_logs_tmpfs:
        # The mountpoint directory would leak into the built images, and it
        # doesn't even make sense for genrule layer construction.
        fail("Genrule layers do not allow setting up a `/logs` tmpfs")

    target_tagger = new_target_tagger()
    image_layer_utils.image_layer_impl(
        _rule_type = "image_genrule_layer_" + rule_type,
        _layer_name = name,
        # Build a new layer. It may be empty.
        _make_subvol_cmd = compile_image_features(
            name = name,
            current_target = image_utils.current_target(name),
            parent_layer = parent_layer,
            features = [target_tagger_to_feature(
                target_tagger,
                struct(genrule_layer = [
                    # TODO: use the `shape.to_dict()` helper from Arnav's diff.
                    shape.as_dict(shape.new(
                        genrule_layer_t,
                        cmd = cmd,
                        user = user,
                        container_opts = container_opts,
                    )),
                ]),
                extra_deps = ["//antlir/bzl:image_genrule_layer"],
            )],
            build_opts = build_opts,
        ),
        **image_layer_kwargs
    )
