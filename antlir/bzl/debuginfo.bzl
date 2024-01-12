# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", antlir2_image = "image")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package", "should_all_images_in_package_use_default_os")
load(":build_defs.bzl", "target_utils")
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
        default_os: str | None = None,
        visibility: _VISIBILITY_OPT = None) -> types.struct:
    """
    Given an OS-like image layer, split it into two images, one of which
    contains the original layer minus any debug symbols and the other _only_ the
    contents of /usr/lib/debug
    """
    layer_name = target_utils.parse_target(layer).name
    stripped_name = stripped_name or (layer_name + ".stripped")
    debuginfo_name = debuginfo_name or (layer_name + ".debuginfo")
    if should_all_images_in_package_use_default_os():
        default_os = default_os or get_default_os_for_package()
    default_os_kwarg = {"default_os": default_os} if default_os else {}
    flavor_kwarg = {"flavor": flavor} if not default_os else {}
    antlir2_image.layer(
        name = stripped_name,
        features = [
            antlir2_feature.remove(path = "/usr/lib/debug", must_exist = False),
        ],
        parent_layer = layer,
        visibility = visibility,
        **(default_os_kwarg | flavor_kwarg)
    )
    cfg_kwargs = {
        "flavor": "//antlir/antlir2/flavor:none",
    } if not default_os else {"default_os": default_os}
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
        visibility = visibility,
        **cfg_kwargs
    )
    return struct(
        stripped = normalize_target(":" + stripped_name),
        debuginfo = normalize_target(":" + debuginfo_name),
    )

debuginfo = struct(
    split = _split,
)
