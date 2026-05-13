# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//cxx:cxx_toolchain_types.bzl", "CxxToolchainInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

SplitBinaryInfo = provider(fields = [
    "stripped",
    "debuginfo",
    "metadata",
    "dwp",
    "resources_stripped",
    "resources_debuginfo",
    "resources_metadata",
])

def _split_binary_impl(ctx: AnalysisContext) -> list[Provider]:
    objcopy = ctx.attrs.objcopy[RunInfo] if ctx.attrs.objcopy else ctx.attrs.cxx_toolchain[CxxToolchainInfo].binary_utilities_info.objcopy

    src = ensure_single_output(ctx.attrs.src)

    src_dwp = None
    maybe_dwp = ctx.attrs.src[DefaultInfo].sub_targets.get("dwp")
    if maybe_dwp:
        src_dwp = ensure_single_output(maybe_dwp[DefaultInfo])

    stripped = ctx.actions.declare_output("stripped", has_content_based_path = False)
    debuginfo = ctx.actions.declare_output("debuginfo", has_content_based_path = False)
    metadata = ctx.actions.declare_output("metadata.json", has_content_based_path = False)

    # TODO(vmagro): Get rid of the empty file fallback
    dwp_out = src_dwp or ctx.actions.write("dwp", "", has_content_based_path = False)

    # Common args for all subcommands
    common_args = cmd_args(
        cmd_args(objcopy, format = "--objcopy={}"),
        cmd_args(src, format = "--binary={}"),
    )

    # Run separate concurrent actions for each output
    ctx.actions.run(
        cmd_args(
            ctx.attrs.debuginfo_splitter[RunInfo],
            "strip",
            common_args,
            "--strip-all" if ctx.attrs.strip_all else cmd_args(),
            cmd_args(stripped.as_output(), format = "--stripped={}"),
        ),
        category = "split",
        identifier = "stripped",
    )

    ctx.actions.run(
        cmd_args(
            ctx.attrs.debuginfo_splitter[RunInfo],
            "debuginfo",
            common_args,
            cmd_args(debuginfo.as_output(), format = "--debuginfo={}"),
        ),
        category = "split",
        identifier = "debuginfo",
    )

    ctx.actions.run(
        cmd_args(
            ctx.attrs.debuginfo_splitter[RunInfo],
            "metadata",
            common_args,
            cmd_args(metadata.as_output(), format = "--metadata={}"),
        ),
        category = "split",
        identifier = "metadata",
    )

    if ctx.attrs.resources_dir:
        resources_dir_src = ctx.attrs.resources_dir
        resources_stripped = ctx.actions.declare_output("resources_stripped", dir = True, has_content_based_path = False)
        resources_debuginfo = ctx.actions.declare_output("resources_debuginfo", dir = True, has_content_based_path = False)
        resources_metadata = ctx.actions.declare_output("resources_metadata", dir = True, has_content_based_path = False)

        ctx.actions.run(
            cmd_args(
                ctx.attrs.debuginfo_splitter[RunInfo],
                "split-dir",
                cmd_args(objcopy, format = "--objcopy={}"),
                "--strip-all" if ctx.attrs.strip_all else cmd_args(),
                cmd_args(resources_dir_src, format = "--input-dir={}"),
                cmd_args(resources_stripped.as_output(), format = "--stripped-dir={}"),
                cmd_args(resources_debuginfo.as_output(), format = "--debuginfo-dir={}"),
                cmd_args(resources_metadata.as_output(), format = "--metadata-dir={}"),
            ),
            category = "split",
            identifier = "resources",
        )
    else:
        resources_stripped = ctx.actions.symlinked_dir("resources_stripped_empty", {}, has_content_based_path = False)
        resources_debuginfo = ctx.actions.symlinked_dir("resources_debuginfo_empty", {}, has_content_based_path = False)
        resources_metadata = ctx.actions.symlinked_dir("resources_metadata_empty", {}, has_content_based_path = False)

    return [
        DefaultInfo(sub_targets = {
            "debuginfo": [DefaultInfo(debuginfo)],
            "dwp": [DefaultInfo(dwp_out)],
            "metadata": [DefaultInfo(metadata)],
            "resources_debuginfo": [DefaultInfo(resources_debuginfo)],
            "resources_metadata": [DefaultInfo(resources_metadata)],
            "resources_stripped": [DefaultInfo(resources_stripped)],
            "stripped": [DefaultInfo(stripped)],
        }),
        SplitBinaryInfo(
            stripped = stripped,
            debuginfo = debuginfo,
            metadata = metadata,
            dwp = dwp_out,
            resources_stripped = resources_stripped,
            resources_debuginfo = resources_debuginfo,
            resources_metadata = resources_metadata,
        ),
    ]

split_binary = anon_rule(
    impl = _split_binary_impl,
    attrs = {
        "cxx_toolchain": attrs.option(attrs.toolchain_dep(default = "toolchains//:cxx", providers = [CxxToolchainInfo]), default = None),
        "debuginfo_splitter": attrs.exec_dep(default = "antlir//antlir/antlir2/tools:debuginfo-splitter"),
        "objcopy": attrs.option(attrs.exec_dep(), default = None),
        "resources_dir": attrs.option(attrs.source(), default = None),
        "src": attrs.dep(),
        "strip_all": attrs.bool(default = False),
    },
    artifact_promise_mappings = {
        "debuginfo": lambda x: x[SplitBinaryInfo].debuginfo,
        "dwp": lambda x: x[SplitBinaryInfo].dwp,
        "metadata": lambda x: x[SplitBinaryInfo].metadata,
        "resources_debuginfo": lambda x: x[SplitBinaryInfo].resources_debuginfo,
        "resources_metadata": lambda x: x[SplitBinaryInfo].resources_metadata,
        "resources_stripped": lambda x: x[SplitBinaryInfo].resources_stripped,
        "src": lambda x: x[SplitBinaryInfo].stripped,
    },
)

def split_binary_anon(
        *,
        ctx: AnalysisContext,
        src: Dependency,
        objcopy: Dependency,
        debuginfo_splitter: Dependency,
        strip_all: bool = False,
        resources_dir = None) -> AnonTarget:
    anon_attrs = {
        "debuginfo_splitter": debuginfo_splitter,
        "name": "debuginfo//" + src.label.package + ":" + src.label.name,
        "objcopy": objcopy,
        "src": src,
        "strip_all": strip_all,
    }
    if resources_dir:
        anon_attrs["resources_dir"] = resources_dir
    return ctx.actions.anon_target(split_binary, anon_attrs)
