# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", antlir2_feature = "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", antlir2_image = "image")
load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")
load(":build_defs.bzl", "target_utils")
load(":image.bzl", antlir1_image = "image")
load(":target_helpers.bzl", "normalize_target")
load(":types.bzl", "types")

_STR_OPT = types.optional(types.str)
_VISIBILITY_OPT = types.optional(types.visibility)

types.lint_noop(_STR_OPT, _VISIBILITY_OPT)

def _split(
        layer: types.label,
        stripped_name: _STR_OPT = None,
        debuginfo_name: _STR_OPT = None,
        flavor: _STR_OPT = None,
        visibility: _VISIBILITY_OPT = None,
        use_antlir2: bool = False) -> types.struct:
    """
    Given an OS-like image layer, split it into two images, one of which
    contains the original layer minus any debug symbols and the other _only_ the
    contents of /usr/lib/debug
    """
    layer_name = target_utils.parse_target(layer).name
    stripped_name = stripped_name or (layer_name + ".stripped")
    debuginfo_name = debuginfo_name or (layer_name + ".debuginfo")
    if use_antlir2:
        antlir2_image.layer(
            name = stripped_name,
            features = [
                antlir2_feature.remove(path = "/usr/lib/debug", must_exist = False),
            ],
            parent_layer = layer,
            visibility = visibility,
        )
        antlir2_image.layer(
            name = debuginfo_name,
            features = [
                antlir2_feature.clone(
                    src_layer = layer,
                    src_path = "/usr/lib/debug",
                    dst_path = "/",
                    user = "root",
                    group = "root",
                ),
            ],
            flavor = "//antlir/antlir2/flavor:none",
            visibility = visibility,
        )
    else:
        antlir1_image.layer(
            name = stripped_name,
            features = [
                antlir1_feature.remove("/usr/lib/debug", must_exist = False),
            ],
            parent_layer = layer,
            flavor = flavor,
            visibility = visibility,
        )
        antlir1_image.layer(
            name = debuginfo_name,
            features = [
                antlir1_feature.clone(layer, "/usr/lib/debug", "/"),
            ],
            flavor = flavor_helpers.get_antlir_linux_flavor(),
            visibility = visibility,
            antlir2 = "debuginfo",
        )
    return struct(
        stripped = normalize_target(":" + stripped_name),
        debuginfo = normalize_target(":" + debuginfo_name),
    )

debuginfo = struct(
    split = _split,
)
