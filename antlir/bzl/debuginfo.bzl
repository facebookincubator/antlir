# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")
load(":build_defs.bzl", "target_utils")
load(":target_helpers.bzl", "normalize_target")

def _split(
        layer: str,
        stripped_name: str | None = None,
        debuginfo_name: str | None = None,
        default_os: str | None = None,
        visibility: list[str] | None = None,
        rootless: bool | None = None) -> struct:
    """
    Given an OS-like image layer, split it into two images, one of which
    contains the original layer minus any debug symbols and the other _only_ the
    contents of /usr/lib/debug
    """
    layer_name = target_utils.parse_target(layer).name
    stripped_name = stripped_name or (layer_name + ".stripped")
    debuginfo_name = debuginfo_name or (layer_name + ".debuginfo")
    default_os = default_os or get_default_os_for_package()
    image.layer(
        name = stripped_name,
        features = [
            feature.remove(path = "/usr/lib/debug", must_exist = False),
        ],
        parent_layer = layer,
        visibility = visibility,
        rootless = rootless,
        default_os = default_os,
    )
    image.layer(
        name = debuginfo_name,
        features = [
            feature.clone(
                src_layer = layer,
                src_path = "/usr/lib/debug",
                dst_path = "/",
                user = "root",
                group = "root",
            ),
        ],
        visibility = visibility,
        rootless = rootless,
        default_os = default_os,
    )
    return struct(
        stripped = normalize_target(":" + stripped_name),
        debuginfo = normalize_target(":" + debuginfo_name),
    )

debuginfo = struct(
    split = _split,
)
