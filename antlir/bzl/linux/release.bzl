# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def _release_file_impl(ctx: AnalysisContext) -> list[Provider]:
    for key in ctx.attrs.api_versions.keys():
        if not key.isupper():
            fail("api_versions keys must be UPPER ({})".format(key))

    rev_time = ctx.actions.declare_output("rev_time.txt")
    if ctx.attrs.vcs_rev_time:
        ctx.actions.run(
            cmd_args(
                "bash",
                "-c",
                cmd_args(
                    "date",
                    "--rfc-3339=seconds",
                    "--utc",
                    "--date",
                    "@" + str(ctx.attrs.vcs_rev_time),
                    ">",
                    rev_time.as_output(),
                    delimiter = " ",
                ),
            ),
            category = "vcs_format_timestamp",
        )
    else:
        ctx.actions.write(rev_time, "1969-12-31 16:00:00-08:00")

    contents_out = ctx.actions.declare_output("os-release")

    def _dyn(ctx, artifacts, outputs, rev_time = rev_time, contents_out = contents_out):
        date, time = artifacts[rev_time].read_string().strip().split(" ")
        rev_time = "{}T{}".format(date, time)

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
            rev = ctx.attrs.vcs_rev or "local",
            rev_time = rev_time,
            api_vers = "\n".join(api_vers),
        ).strip() + "\n"

        ctx.actions.write(outputs[contents_out], contents)

    ctx.actions.dynamic_output(
        dynamic = [rev_time],
        inputs = [],
        outputs = [contents_out.as_output()],
        f = _dyn,
    )

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
            default = {},
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
        "vcs_rev": attrs.option(
            attrs.string(doc = "SCM revision this is being built on"),
            default = None,
        ),
        "vcs_rev_time": attrs.option(
            attrs.int(doc = "Unix timestamp of the commit time"),
            default = None,
        ),
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

def _release_file_macro(
        name: str,
        **kwargs):
    kwargs.setdefault("os_id", selects.or_({
        ("antlir//antlir/antlir2/os:centos8", "antlir//antlir/antlir2/os:centos9"): "centos",
        "antlir//antlir/antlir2/os:eln": "fedora",
    }))
    kwargs.setdefault("os_name", selects.or_({
        ("antlir//antlir/antlir2/os:centos8", "antlir//antlir/antlir2/os:centos9"): "CentOS Stream",
        "antlir//antlir/antlir2/os:eln": "Fedora Linux",
    }))
    eln_version = "40"
    kwargs.setdefault("os_version", select({
        "antlir//antlir/antlir2/os:centos8": "8",
        "antlir//antlir/antlir2/os:centos9": "9",
        "antlir//antlir/antlir2/os:eln": eln_version,
    }))
    kwargs.setdefault("os_version_id", select({
        "antlir//antlir/antlir2/os:centos8": "8",
        "antlir//antlir/antlir2/os:centos9": "9",
        "antlir//antlir/antlir2/os:eln": eln_version,
    }))

    _release_file(
        name = name,
        **(default_target_platform_kwargs() | kwargs)
    )

def _install(
        *,
        path,
        layer,
        variant,
        vcs_rev: str | None = None,
        vcs_rev_time: int | None = None,
        **kwargs):
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

    kwargs.setdefault("ansi_color", "0;34")

    _release_file_macro(
        name = name,
        layer = layer,
        variant = variant,
        vcs_rev = vcs_rev or native.read_config("build_info", "revision", "local"),
        vcs_rev_time = vcs_rev_time or int(native.read_config("build_info", "revision_epochtime", 0)),
        compatible_with = [
            "antlir//antlir/antlir2/os:centos8",
            "antlir//antlir/antlir2/os:centos9",
            "antlir//antlir/antlir2/os:eln",
        ],
        visibility = ["PUBLIC"],
        **kwargs
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
    file = _release_file_macro,
)
