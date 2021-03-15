load("//antlir/bzl:oss_shim.bzl", "buck_command_alias", "buck_genrule", "config")

def generate_tool_wrappers(platform):
    tool_target_prefix = "cxx-toolchain-{}".format(platform)

    tool_wrappers = [
        "ar",
        "gcc",
        "g++",
        "nm",
        "objcopy",
        "ranlib",
        "strip",
    ]

    for tool in tool_wrappers:
        buck_genrule(
            name = "{}-{}".format(tool_target_prefix, tool),
            out = "run",
            bash = """
cat > "$TMP/out" << 'EOF'
#!/bin/bash
set -ue -o pipefail -o noclobber
exec $(exe //tools/build:build) \
--build-layer="$(location {build_appliance})" \
--mode={mode} $@
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
            """.format(
                build_appliance=config.get_build_appliance_for_platform(platform),
                mode=tool,
            ),
            executable=True,
            cacheable=False,
            visibility=["PUBLIC"],
        )

    return tool_target_prefix

def generate_platform_toolchain(platform):
    target_prefix = ":" + generate_tool_wrappers(platform)
    name = target_prefix[1:]

    native.cxx_toolchain(
        name = name,
        compatible_with = [
            "config//runtime:{}".format(platform),
        ],
        # C
        default_target_platform = "config//platform:{}".format(platform),
        c_compiler = "{}-{}".format(target_prefix, "gcc"),
        compiler_type = "gcc",
        # c++
        cxx_compiler = "{}-{}".format(target_prefix, "g++"),
        # Assembly
        assembler = "{}-{}".format(target_prefix, "gcc"),
        # Archive
        archiver = "{}-{}".format(target_prefix, "ar"),
        archiver_type = "gnu",
        # Linker
        linker = "{}-{}".format(target_prefix, "g++"),
        linker_type = "gnu",
        linker_flags = [], # "-Wl,-lpthread"],
        # Other tools
        nm = "{}-{}".format(target_prefix, "nm"),
        objcopy_for_shared_library_interface = "{}-{}".format(target_prefix, "objcopy"),
        ranlib = "{}-{}".format(target_prefix, "ranlib"),
        strip = "{}-{}".format(target_prefix, "strip"),
        # Other configs
        object_file_extension = "o",
        public_headers_symlinks_enabled = True,
        shared_library_extension = "so",
        shared_library_versioned_extension_format = ".so%s",
        shared_library_interface_type = "defined_only",
        static_library_extension = "a",
        use_header_map = False,
        headers_whitelist = [
            "(^|.*)/usr/include/.*",
            "(^|.*)/usr/lib/gcc/.*",
        ],
        visibility = ["PUBLIC"],
    )

    return name

def generate_host_toolchain():
    tool_wrappers = [
        "ar",
        "gcc",
        "g++",
        "nm",
        "objcopy",
        "ranlib",
        "strip",
    ]

    for tool in tool_wrappers:
        buck_genrule(
            name = "cxx-toolchain-host-{}".format(tool),
            out = "run",
            bash = r"""
cat > "$TMP/out" << 'EOF'
#!/bin/bash
set -ue -o pipefail -o noclobber
exec {tool} "$@"
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
            """.format(tool=tool),
            executable = True,
            visibility = ["PUBLIC"],
        )

    target_prefix = ":cxx-toolchain-host"
    name = "cxx-toolchain-host"
    native.cxx_toolchain(
        name = name,
        compatible_with = [
            "config//runtime:linux-x86_64",
        ],
        default_target_platform = "config//platform:linux-x86_64",
        # C
        c_compiler = "{}-{}".format(target_prefix, "gcc"),
        compiler_type = "gcc",
        # c++
        cxx_compiler = "{}-{}".format(target_prefix, "g++"),
        # Assembly
        assembler = "{}-{}".format(target_prefix, "gcc"),
        # Archive
        archiver = "{}-{}".format(target_prefix, "ar"),
        archiver_type = "gnu",
        # Linker
        linker = "{}-{}".format(target_prefix, "g++"),
        linker_type = "gnu",
        linker_flags = [],
        # Other tools
        nm = "{}-{}".format(target_prefix, "nm"),
        objcopy_for_shared_library_interface = "{}-{}".format(target_prefix, "objcopy"),
        ranlib = "{}-{}".format(target_prefix, "ranlib"),
        strip = "{}-{}".format(target_prefix, "strip"),
        # Other configs
        object_file_extension = "o",
        public_headers_symlinks_enabled = True,
        shared_library_extension = "so",
        shared_library_versioned_extension_format = ".so%s",
        shared_library_interface_type = "defined_only",
        static_library_extension = "a",
        use_header_map = False,
        headers_whitelist = [
            "(^|.*)/usr/include/.*",
            "(^|.*)/usr/lib/gcc/.*",
        ],
        visibility = ["PUBLIC"],
    )
    return name

def generate_toolchains():
    toolchain_select = {
        "DEFAULT": ":cxx-toolchain-host",
        "config//runtime:linux-x86_64": ":{}".format(generate_host_toolchain())
    }
    
    for platform in config.get_all_platforms():
        toolchain_select[
            "config//runtime:{}".format(platform)
        ] = ":{}".format(generate_platform_toolchain(platform))

    native.alias(
        name = "toolchain",
        default_target_platform = "config//platform:linux-x86_64",
        actual = select(toolchain_select),
        visibility = ["PUBLIC"],
    )
