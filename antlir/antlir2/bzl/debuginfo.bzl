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

    stripped = ctx.actions.declare_output("stripped")
    debuginfo = ctx.actions.declare_output("debuginfo")
    dwp_out = ctx.actions.declare_output("dwp")
    metadata = ctx.actions.declare_output("metadata.json")

    # objcopy needs a temporary file that it can write to. use a buck2 output
    # artifact so that it doesn't try to put it somewhere it doesn't have access
    # to write
    objcopy_tmp = ctx.actions.declare_output("objcopy_tmp")

    split = ctx.actions.write("split.py", """#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--objcopy", required=True)
parser.add_argument("--binary", required=True, type=Path)
parser.add_argument("--binary-dwp", type=Path)
parser.add_argument("--stripped", required=True, type=Path)
parser.add_argument("--debuginfo", required=True, type=Path)
parser.add_argument("--dwp", required=True, type=Path)
parser.add_argument("--metadata", required=True, type=Path)
parser.add_argument("--objcopy-tmp", required=True, type=Path)

args = parser.parse_args()

# ensure this exists or buck2 will get mad
args.objcopy_tmp.touch()

with open(args.binary, mode="rb") as src_f:
    first_4 = src_f.read(4)
    is_elf = first_4 == b"\x7fELF"

if args.binary_dwp:
    shutil.copyfile(args.binary_dwp, args.dwp)
else:
    with open(args.dwp, "w") as _f:
        pass

# If this is not an ELF binary, it can't be stripped so just copy the original
if not is_elf:
    shutil.copyfile(args.binary, args.stripped)
    with open(args.debuginfo, "w") as _f:
        pass
    with open(args.metadata, "w") as f:
        json.dump({}, f)
    sys.exit(0)

# Save debug symbols to a separate debuginfo file
proc = subprocess.run(
    [
        args.objcopy,
        "--only-keep-debug",
        args.binary,
        args.debuginfo,
    ],
    capture_output=True,
)
if proc.returncode != 0:
    raise RuntimeError("Failed to extract debug symbols for {}:\\n{}\\n{}".format(
        args.binary,
        proc.stdout.decode("utf-8", errors = "surrogateescape"),
        proc.stderr.decode("utf-8", errors = "surrogateescape"),
    ))

# Remove the debug symbols from the stripped binary
proc = subprocess.run(
    [
        args.objcopy,
        "--strip-debug",
        "--remove-section=.pseudo_probe",
        "--remove-section=.pseudo_probe_desc",
        args.binary,
        args.stripped,
    ],
    capture_output=True,
)
if proc.returncode != 0:
    raise RuntimeError("Failed to extract debug symbols for {}:\\n{}\\n{}".format(
        args.binary,
        proc.stdout.decode("utf-8", errors = "surrogateescape"),
        proc.stderr.decode("utf-8", errors = "surrogateescape"),
    ))

# Find the BuildID of the binary. This determines where it should go for gdb to
# look it up under /usr/lib/debug
# https://sourceware.org/gdb/onlinedocs/gdb/Separate-Debug-Files.html
buildid_proc = subprocess.run(
    [
        args.objcopy,
        "--dump-section",
        ".note.gnu.build-id=/dev/stdout",
        args.binary,
        args.objcopy_tmp,
    ],
    capture_output=True,
)
if buildid_proc.returncode != 0:
    raise RuntimeError("Failed to get build-id for {}:\\n{}\\n{}".format(
        args.binary,
        buildid_proc.stdout.decode("utf-8", errors = "surrogateescape"),
        buildid_proc.stderr.decode("utf-8", errors = "surrogateescape"),
    ))
buildid = buildid_proc.stdout

# Prefer to install the debug info by BuildID since it does not require another
# objcopy invocation and is more standard
with open(args.metadata, "w") as f:
    if buildid := buildid[len(buildid) - 20 :].hex():
        json.dump({"buildid": buildid}, f)
    else:
        # Can't setup debuglink here as we don't reliably know the location the binary
        # will end up being placed under, which debuglink relies on, so opt to no-op
        # here and linking will ultimately be handled in the install feature.
        json.dump({}, f)
    """, is_executable = True)

    ctx.actions.run(
        cmd_args(
            split,
            cmd_args(objcopy, format = "--objcopy={}"),
            cmd_args(src, format = "--binary={}"),
            (cmd_args(src_dwp, format = "--binary-dwp={}") if src_dwp else []),
            cmd_args(stripped.as_output(), format = "--stripped={}"),
            cmd_args(debuginfo.as_output(), format = "--debuginfo={}"),
            cmd_args(metadata.as_output(), format = "--metadata={}"),
            cmd_args(dwp_out.as_output(), format = "--dwp={}"),
            cmd_args(objcopy_tmp.as_output(), format = "--objcopy-tmp={}"),
        ),
        category = "split",
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
        "cxx_toolchain": attrs.option(attrs.dep(default = "toolchains//:cxx", providers = [CxxToolchainInfo]), default = None),
        "objcopy": attrs.option(attrs.exec_dep(), default = None),
        "src": attrs.dep(providers = [RunInfo]),
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
        objcopy: Dependency) -> AnonTarget:
    if RunInfo not in src:
        fail("{} does not have a RunInfo provider".format(src.label))
    return ctx.actions.anon_target(split_binary, {
        "name": "debuginfo//" + src.label.package + ":" + src.label.name + ("[{}]".format(src.label.sub_target) if src.label.sub_target else ""),
        "objcopy": objcopy,
        "src": src,
    })
