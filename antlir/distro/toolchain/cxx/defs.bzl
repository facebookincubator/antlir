# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/antlir2/image_command_alias:image_command_alias.bzl", "image_command_alias")
load("//antlir/antlir2/os:oses.bzl", "OSES")

prelude = native

def _layer_tool(tcname: str, tool: str, visibility: list[str] = []) -> str:
    name = tcname + "--" + tool
    if not native.rule_exists(name):
        image_command_alias(
            name = name,
            root = ":{}--root".format(tcname),
            exe = tool,
            rootless = True,
            visibility = visibility,
        )
    return ":" + name

def _include_sysroot(sysroot: str, flag: str) -> list[str]:
    return ["-{}$(location {}){}".format(flag, sysroot, {"I": "/usr/include", "L": "/usr/lib64"}[flag])]

def _single_image_cxx_toolchain(
        *,
        name: str,
        platform_name,
        platform_deps_aliases,
        layer: str,
        sysroot: str = "antlir//antlir/distro/deps:sysroot",
        visibility: list[str] = []):
    package.unprivileged_dir(
        name = name + "--root",
        layer = layer,
        rootless = True,
        dot_meta = False,
    )

    _llvm_base_args = [
        "-target",
        select({
            "ovr_config//cpu:arm64": "aarch64-unknown-linux-gnu",
            "ovr_config//cpu:x86_64": "x86_64-redhat-linux-gnu",
        }),
        "--sysroot=$(location {})".format(sysroot),
    ]

    native.cxx_toolchain(
        name = name,
        platform_name = platform_name,
        platform_deps_aliases = platform_deps_aliases,
        archiver = _layer_tool(name, "llvm-ar"),
        archiver_type = "gnu",
        archiver_flags = _llvm_base_args,
        assembler = _layer_tool(name, "clang"),
        c_compiler = _layer_tool(name, "clang"),
        c_compiler_flags = _llvm_base_args + _include_sysroot(sysroot, "I"),
        c_preprocessor_flags = ["-I$(location {})".format(sysroot)],
        compiler_type = "clang",
        cxx_compiler = _layer_tool(name, "clang++"),
        cxx_compiler_flags = _llvm_base_args + _include_sysroot(sysroot, "I"),
        cxx_preprocessor_flags = [
            "-I$(location {})".format(sysroot),
            # TODO: this may not always be correct, but I cannot get it to work in
            # any permutation of the stdc++ target, so I'm putting the std here
            "-std=c++20",
        ],
        exec_compatible_with = [
            "ovr_config//os:linux",
        ],
        link_ordering = "topological",
        linker = _layer_tool(name, "clang"),
        linker_flags = ["-fuse-ld=lld"] + _llvm_base_args + _include_sysroot(sysroot, "L"),
        linker_type = "gnu",
        generate_linker_maps = False, # @oss-enable
        nm = _layer_tool(name, "nm"),
        objcopy_for_shared_library_interface = _layer_tool(name, "objcopy"),
        requires_archives = True,
        shared_library_interface_type = "disabled",
        strip = _layer_tool(name, "strip"),
        visibility = visibility,
    )

def image_cxx_toolchain(
        *,
        name: str,
        layer: str,
        sysroot: str = "antlir//antlir/distro/deps:sysroot",
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
        _single_image_cxx_toolchain(
            name = "{}--{}".format(name, os.name),
            platform_name = selects.apply(select({
                "ovr_config//cpu:arm64": "aarch64",
                "ovr_config//cpu:x86_64": "x86_64",
            }), lambda arch: os.name + "-" + arch),
            platform_deps_aliases = select({
                "ovr_config//cpu:arm64": ["linux-aarch64"],
                "ovr_config//cpu:x86_64": ["linux-x86_64"],
            }),
            layer = ":{}--{}--layer".format(name, os.name),
            sysroot = sysroot,
            visibility = [],
        )
