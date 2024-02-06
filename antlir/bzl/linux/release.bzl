# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load(":release.buck2.bzl?v2_only", buck2_release_file = "release_file")

def _install(path, layer, os_name, variant, os_version = "9", os_version_id = "9", os_id = "centos", ansi_color = "0;34", api_versions = {}):
    """
    Build an `os-release` file and install it at the provided `path` location.
    See https://www.freedesktop.org/software/systemd/man/os-release.html
    for a detailed description of this from upstream.  The purpose of this API
    is to provide a means to include metadata about the VCS revision and
    buck target of the `image.layer` that this os-release file is being
    installed into.

    `layer`: A relative target path to the layer that the `os-release` file
             will be installed into. It is fully normallized and then inserted
             as the IMAGE_LAYER key.
             Note: the need to include this is an unfortunate wart in the current
             Antlir implementation mainly due to Buck's inability to provide
             context about the target graph when targets are built. Buck2 might
             help solve that core problem, but another approach is to support
             the generation of this file directly in the Compiler itself.

    `os_name`: Populates the NAME key and `os_name.lower()` populates the ID key.
    `variant`: Populates the VARIANT key and `variant.lower()` populates the VARIANT_ID key.
    `ansi_color`: Populates the ANSI_COLOR key.
    `api_versions`: A means of expressing the (preferably monotonically
                    increasing) API version for various MetalOS features
                    embedded in the image. Populates API_VER_{KEY}={val} for
                    each provided pair. Keys must be uppercase alpha and
                    underscores only; values must be integers. Absolute values
                    are intended to be meaningless, but they are integers for
                    easy comparison so we can express things like "only if
                    FOO_API is greater than 11"

    The current VCS rev, as a SHA-1 hash, is captured and the entire IMAGE_VCS_REV key
    and a few others, as describe below.

    The current VCS rev timestamp, in ISO-8601 format, is used as both the VERSION and
    IMAGE_VCS_REV_TIME keys.

    The PRETTY_NAME key is formatted as:
      {os_name} {variant} ({revision})

    """

    if not layer.startswith(":"):
        fail("Please provide `layer` as a relative target name.")

    name = layer[1:] + "__os-release"

    buck2_release_file(
        name = name,
        ansi_color = ansi_color,
        api_versions = api_versions,
        layer = layer,
        os_id = os_id,
        os_name = os_name,
        os_version = os_version,
        os_version_id = os_version_id,
        variant = variant,
        visibility = ["PUBLIC"],
    )

    return [
        antlir2_feature.remove(
            path = path,
            must_exist = False,
        ),
        antlir2_feature.install(
            src = normalize_target(":" + name),
            dst = path,
        ),
    ]

# Exported API
release = struct(
    install = _install,
)
