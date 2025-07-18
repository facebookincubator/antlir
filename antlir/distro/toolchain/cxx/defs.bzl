# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode//tools/build/buck/wrappers:utils.bzl", "nvcc_wrapper")
load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/image_command_alias:image_command_alias.bzl", "image_command_alias")
load("//antlir/antlir2/os:oses.bzl", "OSES")
load("//antlir/distro/toolchain/cuda:defs.bzl", "CUDA_VERSIONS")

prelude = native

def _single_image_cxx_toolchain(
        *,
        name: str,
        platform_name,
        platform_deps_aliases,
        layer: str,
        os: str,
        sysroot: str,
        visibility: list[str] = []):
    def _layer_tool(tool: str, version: str | None = None, **kwargs) -> str:
        tool_name = name + "--" + tool + (("-" + version) if version else "")
        if not native.rule_exists(tool_name):
            image_command_alias(
                name = tool_name,
                layer = kwargs.pop("layer", None) or layer,
                exe = kwargs.pop("exe", None) or tool,
                default_os = os,
                rootless = True,
                visibility = visibility,
                **kwargs
            )
        return ":" + tool_name

    def _cuda_layer_tool(tool: str, version: str, **kwargs) -> str:
        """
        CUDA tools have to run in a dedicated layer that have cuda rpms installed.
        This includes clang! If you run clang from the normal layer it's not going
        to be able to find cuda runtime headers.
        """
        return _layer_tool(
            tool,
            version = version,
            layer = "antlir//antlir/distro/toolchain/cuda:layer-{}".format(version),
            **kwargs
        )

    _llvm_base_args = [
        "-target",
        select({
            "ovr_config//cpu:arm64": "aarch64-unknown-linux-gnu",
            "ovr_config//cpu:x86_64": "x86_64-redhat-linux-gnu",
        }),
        "--sysroot=$(location {})".format(sysroot),
    ] + select({
        "DEFAULT": [],
        # Include arch-specific flags that are set by fbcode cxx toolchains.
        "ovr_config//cpu:x86_64": [
            "-march=haswell",
            "-mtune=skylake",
        ],
    }) + [
        "-fopenmp",
        # Make sure this is passed in because when compilations run on RE we
        # need to force this dir to get mounted into the container.
        "-idirafter",
        "$(location antlir//antlir/distro/deps/glibc:include)",
    ]

    nvcc_wrapper_rule = name + "--nvcc-wrapper"
    if os == "centos9":
        for version in CUDA_VERSIONS:
            nvcc_wrapper(
                name = nvcc_wrapper_rule + "-" + version,
                nvcc_target = _cuda_layer_tool(
                    tool = "nvcc",
                    version = version,
                    exe = "/usr/local/cuda-{}/bin/nvcc".format(version),
                    # wrap_nvcc expects these to be set when it's running in the container.
                    pass_env = ["TMPDIR", "BUCK_SCRATCH_PATH"],
                ),
                gcc_target = _cuda_layer_tool("gcc", version = version),
                clang_target = _cuda_layer_tool("clang", version = version),
                cuda_target = "antlir//antlir/distro/toolchain/cuda:cuda_path-{}".format(version),
                args = [
                    # libshim.so doesn't make it into the container image where invocations run
                    # so ignore it for now at the cost of some non-determinism.
                    "-_OMIT_LIBSHIM_FLAG_",
                ],
            )

    cuda_tools = {}

    # Nvidia repo is only available on centos9.
    if os == "centos9":
        cuda_tools = {
            "cuda_compiler": select({
                "ovr_config//third-party/cuda/constraints:12.4": ":" + nvcc_wrapper_rule + "-12.4",
                "ovr_config//third-party/cuda/constraints:12.8": ":" + nvcc_wrapper_rule + "-12.8",
            }),
            "cuda_compiler_type": "clang",
        }

    native.cxx_toolchain(
        name = name,
        platform_name = platform_name,
        platform_deps_aliases = platform_deps_aliases,
        archiver = _layer_tool("llvm-ar"),
        archiver_type = "gnu",
        archiver_flags = _llvm_base_args,
        assembler = _layer_tool("clang"),
        c_compiler = _layer_tool("clang"),
        c_compiler_flags = _llvm_base_args,
        compiler_type = "clang",
        cxx_compiler = _layer_tool("clang++"),
        cxx_compiler_flags = _llvm_base_args,
        cxx_preprocessor_flags = [
            # TODO: this may not always be correct, but I cannot get it to work in
            # any permutation of the stdc++ target, so I'm putting the std here
            "-std=gnu++20",
        ],
        exec_compatible_with = [
            "ovr_config//os:linux",
        ],
        link_ordering = "topological",
        linker = _layer_tool("clang"),
        linker_flags = [
            # Allow text relocations in the output.  Text sections (i.e. compiled code)
            # may require relocations.  As code segments are  marked as read-only,
            # LLD would not want to modify it (to apply the relocation) by default.
            # We'll allow that; in fact PIC ELFs require this.  Gold has `notext`
            # enabled by default, and BFD ld always allows that; match their
            # behavior.  https://reviews.llvm.org/D30530
            "-Wl,-z,notext",
            # Partial relro
            "-Wl,-z,relro",
            # Garbage collect sections to control binary size (S184081).
            # Size reduction in dynamically linked binaries will be less than that of
            # statically linked binaries, only non-exported symbols could be marked
            # "live" and be eligible for removal.
            "-Wl,--gc-sections",
            # LLD is faster and uses less RAM than GOLD.
            # A Buck target may opt-out of linking with lld by using '-fuse-ld=gold'.
            "-fuse-ld=lld",
            "-nodefaultlibs",
            "-Wl,-nostdlib",
        ] + _llvm_base_args,
        linker_type = "gnu",
        generate_linker_maps = False, # @oss-enable
        nm = _layer_tool("llvm-nm"),
        objcopy_for_shared_library_interface = _layer_tool("objcopy"),
        requires_archives = True,
        shared_library_interface_type = "disabled",
        shared_library_interface_producer = "fbcode//tools/shlib_interfaces:mk_elf_shlib_intf.dotslash",
        strip = _layer_tool("strip"),
        visibility = visibility,
        **cuda_tools
    )

def image_cxx_toolchain(
        *,
        name: str,
        layer: str,
        sysroot: str = "antlir//antlir/distro/toolchain/cxx:sysroot-layer",
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
            os = os.name,
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
