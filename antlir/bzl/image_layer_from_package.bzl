# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:compat.bzl?v2_only", antlir2_compat = "compat")
load("//antlir/antlir2/bzl/image:defs.bzl?v2_only", antlir2_image = "image")
load("//antlir/bzl:build_defs.bzl", "alias", "get_visibility")
load("//antlir/bzl:image_source.bzl", "image_source_to_buck2_src")
load(":constants.bzl", "use_rc_target")
load(":flavor_impl.bzl", "flavor_to_struct")
load(":target_helpers.bzl", "normalize_target")

def image_layer_from_package_helper(
        name,
        format,
        flavor,
        rc_layer,
        image_layer_kwargs,
        antlir2_src):
    target = normalize_target(":" + name)
    if use_rc_target(exact_match = True, target = target) and rc_layer == None:
        fail("{}'s rc build was requested but `rc_layer` is unset!".format(target))

    if use_rc_target(target = target) and rc_layer != None:
        alias(
            name = name,
            antlir_rule = "user-internal",
            layer = rc_layer,
            visibility = get_visibility(image_layer_kwargs.get("visibility")),
        )
    else:
        antlir2_image.prebuilt(
            name = name,
            src = antlir2_src,
            flavor = antlir2_compat.from_antlir1_flavor(flavor),
            format = format,
            visibility = get_visibility(image_layer_kwargs.get("visibility")),
        )

# See the `_image_layer_impl` signature (in `image_layer_utils.bzl`) for all
# other supported kwargs.
def image_layer_from_package(
        name,
        format,
        source = None,
        flavor = None,
        # A sendstream layer does not add any build logic on top of the
        # input, so we treat it as internal to improve CI coverage.

        # The target path of the rc-layer implementation that built this
        # packaged layer.  Used in conjunction with the `-c antlir.rc.layers`
        # config to test changes to packaged layers.
        rc_layer = "__unset__",
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
    if rc_layer == "__unset__":
        fail("rc_layer must be specified or explicitly set to None")
    flavor = flavor_to_struct(flavor)

    buck2_src = image_source_to_buck2_src(source)

    image_layer_from_package_helper(
        name,
        format,
        flavor,
        rc_layer,
        image_layer_kwargs,
        antlir2_src = buck2_src,
    )
