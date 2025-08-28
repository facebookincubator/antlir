# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode//buck2/platform/cxx_toolchains:gen_modes.bzl", "DEV", "OPT", "get_cxx_tool_mode_flags", "get_mode_ldflags")
load("@fbcode//tools/build/buck/wrappers:utils.bzl", "asm_wrapper", "nvcc_wrapper")
load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl:selects.bzl", "selects")
load("//antlir/antlir2/image_command_alias:image_command_alias.bzl", "image_command_alias")
load("//antlir/antlir2/os:oses.bzl", "OSES")
load("//antlir/distro/toolchain/cuda:defs.bzl", "CUDA_VERSIONS")

prelude = native

# C preprocessor flags that get passed to every compiler invocation.
_base_pp_flags = [
    # Denotes this is a TEE build (used in C / C++ to gate out specific code).
    "-DTEE_BUILD",
]

# Linker flags passed to every linker instance
# These are set in buck tool wrappers which we don't use, so define them here.
_base_ldflags = [
    "-nodefaultlibs",
    "-Wl,-nostdlib",
    "-L$(location antlir//antlir/distro/deps/libgcc:gcc-redhat-linux)",
    "-B$(location antlir//antlir/distro/deps/glibc:lib)",
    "-L$(location antlir//antlir/distro/deps/glibc:lib)",
]

# gcc-only flags
# gcc only used for CUDA host compilation on some targets.
_base_gcc_flags = [
    "-idirafter",
    "$(location fbcode//antlir/distro/deps/gcc:include)",
    # glog uses an old-style static assert that trips this warning. We get
    # glog from a distro RPM so it's hard to change. This warning doesn't catch
    # much for us anyway, so let's just disable it.
    "-Wno-unused-local-typedefs",
]

# Clang-only flags
# These are set in the buck tool wrappers which the distro toolchain doesn't use
_base_clang_flags = lambda sysroot: [
    # Make sure clang doesn't use its default configs and pick the wrong gcc.
    "--no-default-config",
    "-target",
    select({
        "ovr_config//cpu:arm64": "aarch64-unknown-linux-gnu",
        "ovr_config//cpu:x86_64": "x86_64-redhat-linux-gnu",
    }),
    "--sysroot=$(location {})".format(sysroot),
    # Make sure these are passed in because when compilations run on RE we
    # need to force this dir to get mounted into the container.
    "-resource-dir",
    "$(location antlir//antlir/distro/deps/llvm-fb:resource-dir)",
    "-idirafter",
    "$(location antlir//antlir/distro/deps/llvm-fb:include)",
    # glog uses an old-style static assert that trips this warning. We get
    # glog from a distro RPM so it's hard to change. This warning doesn't catch
    # much for us anyway, so let's just disable it.
    "-Wno-error=unused-local-typedef",
] + select({
    "DEFAULT": [],
    # std::enable_if name mangling is different between 17 and 19, use this to
    # restore it: https://github.com/llvm/llvm-project/issues/85656
    "ovr_config//toolchain/clang/constraints:19": ["-fclang-abi-compat=17"],
})

# Flags applicable to both gcc and clang.
_base_compiler_flags = [
    # These come from buck wrappers which we don't use, so keep these.
    "-Wno-unused-command-line-argument",
    "-nostdinc",
    "-nostdinc++",
    "-idirafter",
    "$(location antlir//antlir/distro/deps/glibc:include)",
]

# These are antlir toolchain-specific flags which fbocde does _not_ usually set.
_antlir_compiler_flags = []

def _prefix_flag(prefix_flag: str, flags: list[str | Select]) -> list[str | Select]:
    """
    Add the prefix flag before all flags in `flags`.
    """
    out_flags = []
    for flag in flags:
        out_flags.append(prefix_flag)
        out_flags.append(flag)
    return out_flags

def _get_mode_ldflags_select() -> Select:
    """
    Returns a selectified list of ldflags derived from fbcode toolchains to pass to the linker.
    """
    return selects.apply(
        _base_ldflags + select({
            "DEFAULT": get_mode_ldflags(platform = "platform010", cxx_mode = DEV.cxx),
            "ovr_config//build_mode/constraints:opt": get_mode_ldflags(platform = "platform010", cxx_mode = OPT.cxx),
        }),
        # --discard-section is an fb-specific linker flag that antlir's ld.lld
        # doesn't understand.
        lambda flags: [f for f in flags if not "--discard-section" in f],
    )

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
        pass_env = kwargs.pop("pass_env", [])

        # buck-out is bind-mounted into the container for tool execution, but
        # make sure this buck-provided scratch dir is visible in the container.
        if "BUCK_SCRATCH_PATH" not in pass_env:
            pass_env.append("BUCK_SCRATCH_PATH")

        if not native.rule_exists(tool_name):
            image_command_alias(
                name = tool_name,
                layer = kwargs.pop("layer", None) or layer,
                exe = kwargs.pop("exe", None) or tool,
                default_os = os,
                rootless = True,
                visibility = visibility,
                pass_env = pass_env,
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

    generic_compiler_flags = _base_compiler_flags + _antlir_compiler_flags
    clang_compiler_flags = _base_clang_flags(sysroot) + generic_compiler_flags
    gcc_compiler_flags = _base_gcc_flags + generic_compiler_flags

    def _get_cxx_tool_mode_flags_select(**kwargs) -> Select:
        """
        Calls get_cxx_tool_mode_flags() with appropriate arguments to yield commonly-used
        fbcode toolchain flags we pass along to the toolchain.

        This selects between dev / opt mode so we can enable both of those build modes for TEE.
        """
        kwargs = {
            # We only support x86_64 for distro toolchain right now.
            "arch": "x86_64",
            # We only support clang as a compiler in distro toolchain.
            "compiler": "clang",
            # get_cxx_tool_mode_flags mostly just sets gcc-specific flags, but we use clang as the host compiler
            # so ignore this.
            "is_nvcc": False,
            # Default to platform010 for distro builds, used for resolving platform-specific flags.
            "platform": "platform010",
        } | kwargs

        # Only support compilation with clang
        return clang_compiler_flags + select({
            "DEFAULT": get_cxx_tool_mode_flags(cxx_mode = DEV.cxx, **kwargs),
            "ovr_config//build_mode/constraints:opt": get_cxx_tool_mode_flags(cxx_mode = OPT.cxx, **kwargs),
        })

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
                cuda_version = version,
                args = [
                    # libshim.so doesn't make it into the container image where invocations run
                    # so ignore it for now at the cost of some non-determinism.
                    "-_OMIT_LIBSHIM_FLAG_",
                ] + selects.apply(
                    clang_compiler_flags + _base_pp_flags,
                    native.partial(_prefix_flag, "-_NVCC_CLANG_FLAG_"),
                ) + selects.apply(
                    gcc_compiler_flags + _base_pp_flags,
                    native.partial(_prefix_flag, "-_NVCC_GCC_FLAG_"),
                ),
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

    asm_wrapper_rule = name + "--nasm-wrapper"
    asm_wrapper(
        name = asm_wrapper_rule,
        asm_target = _layer_tool("nasm"),
    )

    asm_flags = ["-f", "elf64"]

    native.cxx_toolchain(
        name = name,
        platform_name = platform_name,
        platform_deps_aliases = platform_deps_aliases,
        archiver = _layer_tool("llvm-ar"),
        archiver_type = "gnu",
        archiver_flags = clang_compiler_flags,
        asm_compiler = ":" + asm_wrapper_rule,
        asm_compiler_flags = asm_flags,
        asm_compiler_type = "gcc",
        asm_preprocessor = ":" + asm_wrapper_rule,
        asm_preprocessor_flags = asm_flags,
        asm_preprocessor_type = "gcc",
        assembler = _layer_tool("clang"),
        c_compiler = _layer_tool("clang"),
        c_compiler_flags = _get_cxx_tool_mode_flags_select(pp = False, is_cxx = False),
        c_preprocessor_flags = _base_pp_flags + _get_cxx_tool_mode_flags_select(pp = True, is_cxx = False),
        compiler_type = "clang",
        cxx_compiler = _layer_tool("clang++"),
        cxx_compiler_flags = _get_cxx_tool_mode_flags_select(pp = False, is_cxx = True),
        cxx_preprocessor_flags = _base_pp_flags + _get_cxx_tool_mode_flags_select(pp = True, is_cxx = True),
        exec_compatible_with = [
            "ovr_config//os:linux",
        ],
        link_ordering = "topological",
        linker = _layer_tool("clang"),
        linker_flags = _get_mode_ldflags_select(),
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
