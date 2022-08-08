# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":compile_image_features.bzl", "compile_image_features")
load(":constants.bzl", "REPO_CFG")
load(":image_layer_alias.bzl", "image_layer_alias")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":target_helpers.bzl", "normalize_target")
load(":target_tagger.bzl", "image_source_as_target_tagged_dict", "new_target_tagger", "target_tagger_to_feature")

def image_layer_from_package_helper(
        name,
        format,
        flavor,
        flavor_config_override,
        antlir_rule,
        subvol_name,
        features,
        compile_image_features_fn,
        image_layer_kwargs):
    for bad_kwarg in ["parent_layer", "features"]:
        if bad_kwarg in image_layer_kwargs:
            fail("Unsupported with layer_from_package", bad_kwarg)

    if format not in ["cpio", "sendstream", "tar"]:
        fail("Unsupported format for layer_from_package", format)

    image_layer_utils.image_layer_impl(
        _rule_type = "image_layer_from_package",
        _layer_name = name,
        _make_subvol_cmd = compile_image_features_fn(
            name = name,
            current_target = normalize_target(":" + name),
            parent_layer = None,
            features = features,
            flavor = flavor,
            flavor_config_override = flavor_config_override,
            subvol_name = subvol_name,
        ),
        antlir_rule = antlir_rule,
        **image_layer_kwargs
    )

# See the `_image_layer_impl` signature (in `image_layer_utils.bzl`) for all
# other supported kwargs.
def image_layer_from_package(
        name,
        format,
        source = None,
        flavor = None,
        flavor_config_override = None,
        # A sendstream layer does not add any build logic on top of the
        # input, so we treat it as internal to improve CI coverage.
        antlir_rule = "user-internal",
        # The target path of the rc-layer implementation that built this
        # packaged layer.  Used in conjunction with the `-c antlir.rc.layers`
        # config to test changes to packaged layers.
        rc_layer = None,
        subvol_name = None,
        # Mechanistically, applying a send-stream on top of an existing layer
        # is just a regular `btrfs receive`.  However, the rules in the
        # current `receive` implementation for matching the parent to the
        # stream are kind of awkward, and it's not clear whether they are
        # right for us in Buck.
        **image_layer_kwargs):
    """
    Arguments
    - `format`: The format of the package the layer is created from. Supported
    formats include `sendstream` and `tar`.
    - `name`, `source`, etc: same as on `image_layer.bzl`.
    The only unsupported kwargs are `parent_layer`
    (we'll support incremental sendstreams eventually) and
    `features` (make your changes in a child layer).
    """

    # Look to see if we should build the RC version of this layer instead of
    # the packaged one.
    if normalize_target(":" + name) in REPO_CFG.rc_layers:
        if rc_layer == None:
            fail("{}'s rc build was requested but `rc_layer` is unset!".format(normalize_target(":" + name)))

        image_layer_alias(
            name = name,
            layer = rc_layer,
        )
    else:
        target_tagger = new_target_tagger()
        features = [target_tagger_to_feature(
            target_tagger,
            struct(
                layer_from_package = [{
                    "format": format,
                    "source": image_source_as_target_tagged_dict(
                        target_tagger,
                        source,
                    ),
                }],
            ),
        )]

        image_layer_from_package_helper(
            name,
            format,
            flavor,
            flavor_config_override,
            antlir_rule,
            subvol_name,
            features,
            compile_image_features,
            image_layer_kwargs,
        )
