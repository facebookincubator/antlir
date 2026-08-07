# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/os:oses.bzl", "OSES")
load("//antlir/bzl:internal_external.bzl", "internal_external")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def _release_file_dynamic_impl(
    actions: AnalysisActions,
    rev_time: ArtifactValue,
    contents_out: OutputArtifact,
    os_name: str,
    os_id: str,
    os_version: str,
    os_version_id: str,
    variant: str,
    ansi_color: str,
    image_id: str,
    api_versions: dict,
    layer_raw_target: str,
    vcs_rev: str | None,
    package_name: str,
    package_version: str,
):
    """
    Dynamic action implementation that reads the rev_time and generates
    the os-release file.
    """
    date, time = rev_time.read_string().strip().split(" ")
    rev_time_formatted = "{}T{}".format(date, time)

    api_vers = ['API_VER_{key}="{val}"'.format(key = key, val = val) for key, val in api_versions.items()]

    contents = (
        """
NAME="{os_name}"
ID="{os_id}"
VERSION="{os_version}"
VERSION_ID="{os_version_id}"
PRETTY_NAME="{os_name} {os_version} {variant} ({rev})"
IMAGE_ID="{image_id}"
IMAGE_LAYER="{target}"
IMAGE_VCS_REV="{rev}"
IMAGE_VCS_REV_TIME="{rev_time}"
{IMAGE_PACKAGE_KEY}="{image_package}"
VARIANT="{variant}"
VARIANT_ID="{lower_variant}"
ANSI_COLOR="{ansi_color}"
{api_vers}
        """.format(
            os_name = os_name,
            os_id = os_id,
            os_version = os_version,
            os_version_id = os_version_id,
            variant = variant,
            lower_variant = variant.lower(),
            ansi_color = ansi_color,
            image_id = image_id,
            target = layer_raw_target,
            rev = vcs_rev or "local",
            rev_time = rev_time_formatted,
            api_vers = "\n".join(api_vers),
            IMAGE_PACKAGE_KEY = internal_external(fb = "IMAGE_FBPKG", oss = "IMAGE_PACKAGE"),
            image_package = package_name + ":" + package_version,
        ).strip()
        + "\n"
    )

    actions.write(contents_out, contents)
    return []

_release_file_dynamic = dynamic_actions(
    impl = _release_file_dynamic_impl,
    attrs = {
        "ansi_color": dynattrs.value(str),
        "api_versions": dynattrs.value(dict),
        "contents_out": dynattrs.output(),
        "image_id": dynattrs.value(str),
        "layer_raw_target": dynattrs.value(str),
        "os_id": dynattrs.value(str),
        "os_name": dynattrs.value(str),
        "os_version": dynattrs.value(str),
        "os_version_id": dynattrs.value(str),
        "package_name": dynattrs.value(str),
        "package_version": dynattrs.value(str),
        "rev_time": dynattrs.artifact_value(),
        "variant": dynattrs.value(str),
        "vcs_rev": dynattrs.value(str | None),
    },
)

def _release_file_impl(ctx: AnalysisContext) -> list[Provider]:
    for key in ctx.attrs.api_versions.keys():
        if not key.isupper():
            fail("api_versions keys must be UPPER ({})".format(key))

    rev_time = ctx.actions.declare_output("rev_time.txt", has_content_based_path = False)
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

    contents_out = ctx.actions.declare_output("os-release", has_content_based_path = False)

    ctx.actions.dynamic_output_new(
        _release_file_dynamic(
            rev_time = rev_time,
            contents_out = contents_out.as_output(),
            image_id = native.read_root_config("build_info", "target_path", "local"),
            os_name = ctx.attrs.os_name,
            os_id = ctx.attrs.os_id,
            os_version = ctx.attrs.os_version,
            os_version_id = ctx.attrs.os_version_id,
            variant = ctx.attrs.variant,
            ansi_color = ctx.attrs.ansi_color,
            api_versions = ctx.attrs.api_versions,
            layer_raw_target = str(ctx.attrs.layer.raw_target()),
            vcs_rev = ctx.attrs.vcs_rev,
            package_name = ctx.attrs.package_name,
            package_version = ctx.attrs.package_version,
        ),
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
        "layer": attrs.label(
            doc = """
            Layer that the `os-release` file will be installed into. It is fully
            normalized and then inserted as the IMAGE_LAYER key.

            Note: the need to include this is an unfortunate wart in the current
            Antlir implementation mainly due to the way this target is a
            dependency of the image layer.
        """
        ),
        "os_id": attrs.string(),
        "os_name": attrs.string(),
        "os_version": attrs.string(),
        "os_version_id": attrs.string(),
        "package_name": attrs.string(),
        "package_version": attrs.string(),
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
    supports_incoming_transition = True,
)

def _release_file_macro(name: str, **kwargs):
    kwargs.setdefault("ansi_color", "0;34")

    kwargs.setdefault(
        "os_id",
        selects.or_({
            ("antlir//antlir/antlir2/os:centos9", "antlir//antlir/antlir2/os:centos10"): "centos",
            "antlir//antlir/antlir2/os/family:family[debian]": "debian",
            "antlir//antlir/antlir2/os:eln": "fedora",
            "antlir//antlir/antlir2/os:none": "none",
        }),
    )
    kwargs.setdefault(
        "os_name",
        selects.or_({
            ("antlir//antlir/antlir2/os:centos9", "antlir//antlir/antlir2/os:centos10"): "CentOS Stream",
            "antlir//antlir/antlir2/os/family:family[debian]": "Debian GNU/Linux",
            "antlir//antlir/antlir2/os:eln": "Fedora Linux",
            "antlir//antlir/antlir2/os:none": "None",
        }),
    )
    eln_version = "40"
    kwargs.setdefault(
        "os_version",
        select({
            "antlir//antlir/antlir2/os:centos10": "10",
            "antlir//antlir/antlir2/os:centos9": "9",
            "antlir//antlir/antlir2/os:debian-trixie": "13 (Trixie)",
            "antlir//antlir/antlir2/os:eln": eln_version,
            "antlir//antlir/antlir2/os:none": "0",
        }),
    )
    kwargs.setdefault(
        "os_version_id",
        select({
            "antlir//antlir/antlir2/os:centos10": "10",
            "antlir//antlir/antlir2/os:centos9": "9",
            "antlir//antlir/antlir2/os:debian-trixie": "13",
            "antlir//antlir/antlir2/os:eln": eln_version,
            "antlir//antlir/antlir2/os:none": "0",
        }),
    )

    kwargs.setdefault("vcs_rev", native.read_root_config("build_info", "revision", "local"))
    kwargs.setdefault("vcs_rev_time", int(native.read_root_config("build_info", "revision_epochtime", "0")))
    kwargs.setdefault("package_name", native.read_root_config("build_info", "package_name", "<none>"))
    kwargs.setdefault("package_version", native.read_root_config("build_info", "package_version", "local"))

    _release_file(name = name, **(default_target_platform_kwargs() | kwargs))

def _install(*, layer, variant, path: str = "/etc/os-release", **kwargs):
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
        layer = layer,
        variant = variant,
        compatible_with = [os.select_key for os in OSES],
        incoming_transition = "antlir//antlir/antlir2/os/transition:default-to-none",
        visibility = ["PUBLIC"],
        **kwargs,
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
