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

    return [
        DefaultInfo(sub_targets = {
            "debuginfo": [DefaultInfo(debuginfo)],
            "dwp": [DefaultInfo(dwp_out)],
            "metadata": [DefaultInfo(metadata)],
            "stripped": [DefaultInfo(stripped)],
        }),
        SplitBinaryInfo(
            stripped = stripped,
            debuginfo = debuginfo,
            metadata = metadata,
            dwp = dwp_out,
        ),
    ]

split_binary = anon_rule(
    impl = _split_binary_impl,
    attrs = {
        "cxx_toolchain": attrs.option(attrs.toolchain_dep(default = "toolchains//:cxx", providers = [CxxToolchainInfo]), default = None),
        "debuginfo_splitter": attrs.exec_dep(default = "antlir//antlir/antlir2/tools:debuginfo-splitter"),
        "objcopy": attrs.option(attrs.exec_dep(), default = None),
        "src": attrs.dep(),
        "strip_all": attrs.bool(default = False),
    },
    artifact_promise_mappings = {
        "debuginfo": lambda x: x[SplitBinaryInfo].debuginfo,
        "dwp": lambda x: x[SplitBinaryInfo].dwp,
        "metadata": lambda x: x[SplitBinaryInfo].metadata,
        "src": lambda x: x[SplitBinaryInfo].stripped,
    },
)

def split_binary_anon(
        *,
        ctx: AnalysisContext,
        src: Dependency,
        objcopy: Dependency,
        debuginfo_splitter: Dependency,
        strip_all: bool = False) -> AnonTarget:
    return ctx.actions.anon_target(split_binary, {
        "debuginfo_splitter": debuginfo_splitter,
        "name": "debuginfo//" + src.label.package + ":" + src.label.name,
        "objcopy": objcopy,
        "src": src,
        "strip_all": strip_all,
    })
