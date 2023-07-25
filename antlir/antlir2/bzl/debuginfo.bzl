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
])

def _split_binary_impl(ctx: "context") -> ["provider"]:
    objcopy = ctx.attrs.objcopy[RunInfo] if ctx.attrs.objcopy else ctx.attrs.cxx_toolchain[CxxToolchainInfo].binary_utilities_info.objcopy

    src = ensure_single_output(ctx.attrs.src)

    stripped = ctx.actions.declare_output("stripped")
    debuginfo = ctx.actions.declare_output("debuginfo")
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
parser.add_argument("--stripped", required=True, type=Path)
parser.add_argument("--debuginfo", required=True, type=Path)
parser.add_argument("--metadata", required=True, type=Path)
parser.add_argument("--objcopy-tmp", required=True, type=Path)

args = parser.parse_args()

# ensure this exists or buck2 will get mad
args.objcopy_tmp.touch()

with open(args.binary, mode="rb") as src_f:
    first_4 = src_f.read(4)
    is_elf = first_4 == b"\x7fELF"

# If this is not an ELF binary, it can't be stripped so just copy the original
if not is_elf:
    shutil.copyfile(args.binary, args.stripped)
    with open(args.debuginfo, "w") as _f:
        pass
    with open(args.metadata, "w") as f:
        json.dump({"elf": False}, f)
    sys.exit(0)

# Save debug symbols to a separate debuginfo file
subprocess.run(
    [
        args.objcopy,
        "--only-keep-debug",
        args.binary,
        args.debuginfo,
    ],
    check=True,
)

# Remove the debug symbols from the stripped binary
subprocess.run(
    [
        args.objcopy,
        "--strip-debug",
        "--remove-section=.pseudo_probe",
        "--remove-section=.pseudo_probe_desc",
        args.binary,
        args.stripped,
    ],
    check=True,
)

# Find the BuildID of the binary. This determines where it should go for gdb to
# look it up under /usr/lib/debug
# https://sourceware.org/gdb/onlinedocs/gdb/Separate-Debug-Files.html
buildid = subprocess.run(
    [
        args.objcopy,
        "--dump-section",
        ".note.gnu.build-id=/dev/stdout",
        args.binary,
        args.objcopy_tmp,
    ],
    capture_output=True,
    check=True,
).stdout

# Prefer to install the debug info by BuildID since it does not require another
# objcopy invocation and is more standard
if buildid:
    buildid = buildid[len(buildid) - 20 :].hex()
    with open(args.metadata, "w") as f:
        json.dump({"elf": True, "buildid": buildid}, f)
else:
    # We might not be able to get the BuildID if this is a dev-mode binary, so
    # fallback to setting the debuglink section in the stripped binary.
    # This tells the debugger that there is a debug file available, and records
    # the hash of the debuginfo file so that it can be loaded correctly.
    subprocess.run(
        [
            os.path.abspath(args.objcopy),
            f"--add-gnu-debuglink={args.debuginfo.name}",
            args.stripped.resolve(),
        ],
        cwd=args.stripped.parent,
        check=True,
    )
    with open(args.metadata, "w") as f:
        json.dump({"elf": True, "buildid": None}, f)
    """, is_executable = True)

    ctx.actions.run(
        cmd_args(
            split,
            cmd_args(objcopy, format = "--objcopy={}"),
            cmd_args(src, format = "--binary={}"),
            cmd_args(stripped.as_output(), format = "--stripped={}"),
            cmd_args(debuginfo.as_output(), format = "--debuginfo={}"),
            cmd_args(metadata.as_output(), format = "--metadata={}"),
            cmd_args(objcopy_tmp.as_output(), format = "--objcopy-tmp={}"),
        ),
        category = "split",
    )

    return [
        DefaultInfo(sub_targets = {
            "debuginfo": [DefaultInfo(debuginfo)],
            "metadata": [DefaultInfo(metadata)],
            "stripped": [DefaultInfo(stripped)],
        }),
        SplitBinaryInfo(
            stripped = stripped,
            debuginfo = debuginfo,
            metadata = metadata,
        ),
    ]

split_binary = rule(
    impl = _split_binary_impl,
    attrs = {
        "cxx_toolchain": attrs.option(attrs.dep(default = "toolchains//:cxx", providers = [CxxToolchainInfo]), default = None),
        "objcopy": attrs.option(attrs.exec_dep(), default = None),
        "src": attrs.dep(providers = [RunInfo]),
    },
)
