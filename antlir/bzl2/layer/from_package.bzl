# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:flavor_impl.bzl", "flavor_to_struct")
load(
    "//antlir/bzl:image_layer_from_package.bzl",
    "image_layer_from_package_helper",
)
load("//antlir/bzl:image_source.bzl", "image_source")
load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.shape.bzl", image_source_t = "target_tagged_image_source_t")
load("//antlir/bzl2:compile_image_features.bzl", "compile_image_features")
load(
    "//antlir/bzl2:feature_rule.bzl",
    "maybe_add_feature_rule",
)
load(
    "//antlir/bzl2:image_source_helper.bzl",
    "normalize_target_and_mark_path_in_source_dict",
)
load(":from_package.shape.bzl", "layer_from_package_t")

# See the `_image_layer_impl` signature (in `image_layer_utils.bzl`) for all
# other supported kwargs.
def layer_from_package(
        name,
        format,
        source = None,
        flavor = None,
        flavor_config_override = None,
        # A sendstream layer does not add any build logic on top of the
        # input, so we treat it as internal to improve CI coverage.
        antlir_rule = "user-internal",
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
    flavor = flavor_to_struct(flavor)

    source_dict = shape.as_dict_shallow(image_source(maybe_export_file(source)))
    source_dict, normalized_target = \
        normalize_target_and_mark_path_in_source_dict(source_dict)

    # copy in buck1 version
    features = [maybe_add_feature_rule(
        name = "layer_from_package",
        include_in_target_name = {"name": name},
        feature_shape = layer_from_package_t(
            format = format,
            source = image_source_t(**source_dict),
        ),
        deps = [normalized_target],
    )]

    image_layer_from_package_helper(
        name,
        format,
        flavor,
        flavor_config_override,
        antlir_rule,
        rc_layer,
        subvol_name,
        features,
        compile_image_features,
        image_layer_kwargs,
    )
