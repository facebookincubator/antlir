# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "@prelude//python:toolchain.bzl",
    "PythonPlatformInfo",
    "PythonToolchainInfo",
)
load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/antlir2/image_command_alias:image_command_alias.bzl", "image_command_alias")
load("//antlir/antlir2/os:oses.bzl", "OSES")

prelude = native

def _layer_tool(tcname: str, tool: str, os: str, visibility: list[str] = []) -> str:
    name = tcname + "--" + tool
    if not native.rule_exists(name):
        image_command_alias(
            name = name,
            root = ":{}--root".format(tcname),
            exe = tool,
            default_os = os,
            rootless = True,
            pass_env = ["PYTHONPATH"],
            visibility = visibility,
        )
    return ":" + name

def _single_image_python_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        PythonToolchainInfo(
            build_standalone_binaries_locally = True,
            compile = ctx.attrs.compile,
            host_interpreter = ctx.attrs.host_python[RunInfo],
            interpreter = RunInfo(cmd_args(ctx.attrs.interpreter)),
            package_style = "standalone",
            pex_extension = ".pex",
            version = ctx.attrs.python_version,
        ),
        PythonPlatformInfo(name = ctx.attrs.platform_name),
    ]

_single_image_python_toolchain = rule(
    impl = _single_image_python_toolchain_impl,
    attrs = {
        "compile": attrs.default_only(attrs.source(default = "prelude//python/tools:compile.py")),
        "host_python": attrs.exec_dep(),
        "interpreter": attrs.string(default = "python3"),
        "platform_name": attrs.string(),
        "python_version": attrs.option(attrs.string(), default = None),
    },
    is_toolchain_rule = True,
)

def image_python_toolchain(
        *,
        name: str,
        layer: str,
        visibility: list[str] = []):
    oses = [os for os in OSES if os.has_platform_toolchain]

    # The "real" toolchain is actually an alias that depends on the selected OS.
    # This is necessary because all the tools listed above (clang, ld.lld, etc)
    # are exec_deps which do not inherit the target configuration, but we want
    # them to match the target platform! As a workaround, we select the entire
    # toolchain with "pre-configured" exec_deps that match the target os version
    # (but maybe not the target os architecture!)
    prelude.toolchain_alias(
        name = name,
        actual = select(
            {
                os.select_key: ":{}--{}".format(name, os.name)
                for os in oses
            } |
            # This will never actually be configured as DEFAULT for a real
            # build, but to keep tooling that expects 'cquery' to work (which is
            # very reasonable), just arbitrarily choose the first os to use as
            # the default when looking up this target directly (instead of
            # preconfigured as a dependency of something using an antlir
            # distro platform)
            {"DEFAULT": ":{}--{}".format(name, oses[0].name)},
        ),
        visibility = visibility,
    )

    for os in oses:
        antlir2_configured_alias(
            name = "{}--{}--layer".format(name, os.name),
            actual = layer,
            default_os = os.name,
        )
        package.unprivileged_dir(
            name = "{}--{}--root".format(name, os.name),
            layer = ":{}--{}--layer".format(name, os.name),
            rootless = True,
            dot_meta = False,
        )
        _single_image_python_toolchain(
            name = "{}--{}".format(name, os.name),
            host_python = select(
                {
                    os.select_key: _layer_tool("{}--{}".format(name, os.name), os.python.interpreter, os.name)
                    for os in oses
                } |
                # See above comment about DEFAULT
                {"DEFAULT": _layer_tool("{}--{}".format(name, oses[0].name), oses[0].python.interpreter, os.name)},
            ),
            interpreter = select({
                os.select_key: os.python.interpreter
                for os in oses
            } | {"DEFAULT": "python3"}),
            platform_name = selects.apply(select({
                "ovr_config//cpu:arm64": "aarch64",
                "ovr_config//cpu:x86_64": "x86_64",
            }), lambda arch: os.name + "-" + arch),
            python_version = select({
                os.select_key: os.python.version_str
                for os in oses
            } | {"DEFAULT": oses[0].python.version_str}),
            visibility = [],
        )
