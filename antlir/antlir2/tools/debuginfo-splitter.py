#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import json
import shutil
import subprocess
from pathlib import Path


def is_elf_binary(binary: Path) -> bool:
    """Check if the binary is an ELF file."""
    if binary.is_dir():
        return False
    with open(binary, mode="rb") as src_f:
        first_4 = src_f.read(4)
        return first_4 == b"\x7fELF"


def cmd_strip(args: argparse.Namespace) -> None:
    """Generate the stripped binary."""
    if not is_elf_binary(args.binary):
        # If this is not an ELF binary, it can't be stripped so just copy the original
        if args.binary.is_dir():
            shutil.copytree(args.binary, args.stripped, symlinks=True)
        else:
            shutil.copy2(args.binary, args.stripped)
        return

    # Remove symbols from the stripped binary.
    # --strip-all removes all symbols (.symtab, .strtab) in addition to debug
    # sections, producing a significantly smaller binary.
    # --strip-debug only removes DWARF debug sections but preserves .symtab
    # and .strtab for symbolication.
    if args.strip_all:
        strip_flags = ["--strip-all"]
    else:
        strip_flags = ["--strip-debug", "--keep-file-symbols"]

    proc = subprocess.run(
        [
            args.objcopy,
        ]
        + strip_flags
        + [
            "--remove-section=.pseudo_probe",
            "--remove-section=.pseudo_probe_desc",
            args.binary,
            args.stripped,
        ],
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            "Failed to strip debug symbols for {}:\\n{}\\n{}".format(
                args.binary,
                proc.stdout.decode("utf-8", errors="surrogateescape"),
                proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )


def cmd_debuginfo(args: argparse.Namespace) -> None:
    """Generate the debuginfo file."""
    if not is_elf_binary(args.binary):
        # If this is not an ELF binary, create an empty debuginfo file
        with open(args.debuginfo, "w"):
            pass
        return

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
        raise RuntimeError(
            "Failed to extract debug symbols for {}:\\n{}\\n{}".format(
                args.binary,
                proc.stdout.decode("utf-8", errors="surrogateescape"),
                proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )


def cmd_metadata(args: argparse.Namespace) -> None:
    """Generate the metadata.json file."""
    if not is_elf_binary(args.binary):
        with open(args.metadata, "w") as f:
            json.dump({}, f)
        return

    # Find the BuildID of the binary. This determines where it should go for gdb to
    # look it up under /usr/lib/debug
    # https://sourceware.org/gdb/onlinedocs/gdb/Separate-Debug-Files.html
    buildid_proc = subprocess.run(
        [
            args.objcopy,
            "-O",
            "binary",
            "--only-section",
            ".note.gnu.build-id",
            args.binary,
            "/dev/stdout",
        ],
        capture_output=True,
    )
    if buildid_proc.returncode != 0:
        raise RuntimeError(
            "Failed to get build-id for {}:\\n{}\\n{}".format(
                args.binary,
                buildid_proc.stdout.decode("utf-8", errors="surrogateescape"),
                buildid_proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )
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


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(required=True)

    # Common arguments shared by subcommands
    common_parser = argparse.ArgumentParser(add_help=False)
    common_parser.add_argument("--objcopy", required=True)
    common_parser.add_argument("--binary", required=True, type=Path)

    strip_parser = subparsers.add_parser("strip", parents=[common_parser])
    strip_parser.add_argument("--stripped", required=True, type=Path)
    strip_parser.add_argument(
        "--strip-all",
        action="store_true",
        default=False,
        help="Use --strip-all instead of --strip-debug to also remove .symtab and .strtab",
    )
    strip_parser.set_defaults(func=cmd_strip)

    debuginfo_parser = subparsers.add_parser("debuginfo", parents=[common_parser])
    debuginfo_parser.add_argument("--debuginfo", required=True, type=Path)
    debuginfo_parser.set_defaults(func=cmd_debuginfo)

    metadata_parser = subparsers.add_parser("metadata", parents=[common_parser])
    metadata_parser.add_argument("--metadata", required=True, type=Path)
    metadata_parser.set_defaults(func=cmd_metadata)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
