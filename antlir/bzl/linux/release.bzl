# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "normalize_target")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _install(path, layer, os_name, variant, ansi_color = "0;34", api_versions = {}):
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

    if not types.is_dict(api_versions) or any(
        [
            (
                not types.is_string(k) or
                not types.is_int(v) or
                k.islower() or
                not k.replace("_", "Z").isalpha() or
                k.startswith("_") or
                k.endswith("_")
            )
            for k, v in api_versions.items()
        ],
    ):
        fail(
            "api_versions must be specified as a dictionary of uppercase keys and int values",
        )

    api_vers = "\n".join(
        sorted(
            ['API_VER_{}="{}"'.format(key, val) for key, val in api_versions.items()],
        ),
    )

    buck_genrule(
        name = name,
        bash = r"""
set -x -eu

rev="`$(exe {vcs}) --rev`"
rev_time="`$(exe {vcs}) --timestamp`"

cat > $OUT << EOF
NAME="{os_name}"
ID="{os_name_lower}"
VERSION="$rev_time"
PRETTY_NAME="{os_name} {variant} ($rev)"
IMAGE_ID="{image_id}"
IMAGE_LAYER="{target}"
IMAGE_VCS_REV="$rev"
IMAGE_VCS_REV_TIME="$rev_time"
VARIANT="{variant}"
VARIANT_ID="{lower_variant}"
ANSI_COLOR="{ansi_color}"
{api_vers}EOF
        """.format(
            ansi_color = ansi_color,
            image_id = native.read_config("build_info", "target_path", "local"),
            target = normalize_target(layer),
            os_name = os_name,
            os_name_lower = os_name.lower(),
            lower_variant = variant.lower(),
            variant = variant,
            vcs = antlir_dep(":vcs"),
            # Ensure no blank lines are added if api_vers is empty
            api_vers = api_vers + "\n" if api_vers else "",
        ),
        antlir_rule = "user-internal",
    )

    return [
        feature.remove(path, must_exist = False),
        feature.install(normalize_target(":" + name), path),
    ]

# Exported API
release = struct(
    install = _install,
)
