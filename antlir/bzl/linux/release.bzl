# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", "feature")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "normalize_target")

def _release_file_impl(ctx: AnalysisContext) -> list[Provider]:
    for key in ctx.attrs.api_versions.keys():
        if not key.isupper():
            fail("api_versions keys must be UPPER ({})".format(key))

    vcs_info = ctx.actions.declare_output("vcs.json")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._vcs[RunInfo],
            cmd_args(vcs_info.as_output(), format = "--json={}"),
        ),
        category = "vcs_info",
        local_only = True,  # uses hg
    )

    contents_out = ctx.actions.declare_output("os-release")

    def _dyn(ctx, artifacts, outputs, vcs_info = vcs_info, contents_out = contents_out):
        vcs_info = artifacts[vcs_info].read_json()

        api_vers = [
            "API_VER_{key}=\"{val}\"".format(key = key, val = val)
            for key, val in ctx.attrs.api_versions.items()
        ]

        contents = """
NAME="{os_name}"
ID="{os_id}"
VERSION="{os_version}"
VERSION_ID="{os_version_id}"
PRETTY_NAME="{os_name} {os_version} {variant} ({rev})"
IMAGE_ID="{image_id}"
IMAGE_LAYER="{target}"
IMAGE_VCS_REV="{rev}"
IMAGE_VCS_REV_TIME="{rev_time}"
VARIANT="{variant}"
VARIANT_ID="{lower_variant}"
ANSI_COLOR="{ansi_color}"
{api_vers}
        """.format(
            os_name = ctx.attrs.os_name,
            os_id = ctx.attrs.os_id,
            os_version = ctx.attrs.os_version,
            os_version_id = ctx.attrs.os_version_id,
            variant = ctx.attrs.variant,
            lower_variant = ctx.attrs.variant.lower(),
            ansi_color = ctx.attrs.ansi_color,
            image_id = native.read_config("build_info", "target_path", "local"),
            target = ctx.attrs.layer.raw_target(),
            rev = vcs_info["rev_id"],
            rev_time = vcs_info["rev_timestamp_iso8601"],
            api_vers = "\n".join(api_vers),
        ).strip() + "\n"

        ctx.actions.write(outputs[contents_out], contents)

    ctx.actions.dynamic_output(dynamic = [vcs_info], inputs = [], outputs = [contents_out], f = _dyn)

    return [
        DefaultInfo(contents_out),
    ]

_release_file = rule(
    impl = _release_file_impl,
    attrs = {
        "ansi_color": attrs.string(default = "0;34"),
        "api_versions": attrs.dict(
            attrs.string(),
            attrs.int(),
            doc = """
                A means of expressing the (preferably monotonically increasing)
                API version for various MetalOS features embedded in the image.
                Populates API_VER_{KEY}={val} for each provided pair. Keys must
                be uppercase alpha and underscores only; values must be
                integers.
                Absolute values are intended to be meaningless, but they are
                integers for easy comparison so we can express things like "only
                if FOO_API is greater than 11"
            """,
        ),
        "layer": attrs.label(doc = """
            Layer that the `os-release` file will be installed into. It is fully
            normalized and then inserted as the IMAGE_LAYER key.

            Note: the need to include this is an unfortunate wart in the current
            Antlir implementation mainly due to the way this target is a
            dependency of the image layer.
        """),
        "os_id": attrs.string(),
        "os_name": attrs.string(),
        "os_version": attrs.string(),
        "os_version_id": attrs.string(),
        "variant": attrs.string(),
        "_vcs": attrs.default_only(attrs.exec_dep(default = antlir_dep(":vcs"))),
    },
    doc = """
        Build an `os-release` file.
        See https://www.freedesktop.org/software/systemd/man/os-release.html
        for a detailed description of this from upstream.  The purpose of this API
        is to provide a means to include metadata about the VCS revision and buck
        target of the `image.layer` that this os-release file is being installed
        into.

        The current VCS rev, as a SHA-1 hash, is captured and the entire IMAGE_VCS_REV key
        and a few others, as describe below.

        The current VCS rev timestamp, in ISO-8601 format, is used as both the VERSION and
        IMAGE_VCS_REV_TIME keys.

        The PRETTY_NAME key is formatted as:
        {os_name} {variant} ({revision})
    """,
)

_release_file_macro = rule_with_default_target_platform(_release_file)

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

    _release_file_macro(
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
        feature.remove(
            path = path,
            must_exist = False,
        ),
        feature.install(
            src = normalize_target(":" + name),
            dst = path,
        ),
    ]

# Exported API
release = struct(
    install = _install,
    file = _release_file,
)
