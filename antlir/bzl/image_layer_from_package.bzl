# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":compile_image_features.bzl", "compile_image_features")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":image_utils.bzl", "image_utils")
load(":target_tagger.bzl", "image_source_as_target_tagged_dict", "new_target_tagger", "target_tagger_to_feature")

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
    for bad_kwarg in ["parent_layer", "features"]:
        if bad_kwarg in image_layer_kwargs:
            fail("Unsupported with layer_from_package", bad_kwarg)

    if format not in ["cpio", "sendstream", "tar"]:
        fail("Unsupported format for layer_from_package", format)

    target_tagger = new_target_tagger()
    image_layer_utils.image_layer_impl(
        _rule_type = "image_layer_from_package",
        _layer_name = name,
        _make_subvol_cmd = compile_image_features(
            name = name,
            current_target = image_utils.current_target(name),
            parent_layer = None,
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
            )],
            flavor = flavor,
            flavor_config_override = flavor_config_override,
            subvol_name = subvol_name,
        ),
        antlir_rule = antlir_rule,
        **image_layer_kwargs
    )
