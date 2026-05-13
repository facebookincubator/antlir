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


def strip_binary(binary: Path, objcopy: str, stripped: Path, strip_all: bool) -> None:
    if not is_elf_binary(binary):
        if binary.is_dir():
            shutil.copytree(binary, stripped, symlinks=True)
        else:
            shutil.copy2(binary, stripped)
        return

    # --strip-all removes all symbols (.symtab, .strtab) in addition to debug
    # sections, producing a significantly smaller binary.
    # --strip-debug only removes DWARF debug sections but preserves .symtab
    # and .strtab for symbolication.
    if strip_all:
        strip_flags = ["--strip-all"]
    else:
        strip_flags = ["--strip-debug", "--keep-file-symbols"]

    proc = subprocess.run(
        [objcopy]
        + strip_flags
        + [
            "--remove-section=.pseudo_probe",
            "--remove-section=.pseudo_probe_desc",
            binary,
            stripped,
        ],
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            "Failed to strip debug symbols for {}:\\n{}\\n{}".format(
                binary,
                proc.stdout.decode("utf-8", errors="surrogateescape"),
                proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )


def cmd_strip(args: argparse.Namespace) -> None:
    strip_binary(args.binary, args.objcopy, args.stripped, args.strip_all)


def extract_debuginfo(binary: Path, objcopy: str, debuginfo: Path) -> None:
    if not is_elf_binary(binary):
        with open(debuginfo, "w"):
            pass
        return

    proc = subprocess.run(
        [objcopy, "--only-keep-debug", binary, debuginfo],
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            "Failed to extract debug symbols for {}:\\n{}\\n{}".format(
                binary,
                proc.stdout.decode("utf-8", errors="surrogateescape"),
                proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )


def cmd_debuginfo(args: argparse.Namespace) -> None:
    extract_debuginfo(args.binary, args.objcopy, args.debuginfo)


def write_metadata(binary: Path, objcopy: str, metadata: Path) -> None:
    if not is_elf_binary(binary):
        with open(metadata, "w") as f:
            json.dump({}, f)
        return

    # Find the BuildID of the binary. This determines where it should go for gdb to
    # look it up under /usr/lib/debug
    # https://sourceware.org/gdb/onlinedocs/gdb/Separate-Debug-Files.html
    buildid_proc = subprocess.run(
        [
            objcopy,
            "-O",
            "binary",
            "--only-section",
            ".note.gnu.build-id",
            binary,
            "/dev/stdout",
        ],
        capture_output=True,
    )
    if buildid_proc.returncode != 0:
        raise RuntimeError(
            "Failed to get build-id for {}:\\n{}\\n{}".format(
                binary,
                buildid_proc.stdout.decode("utf-8", errors="surrogateescape"),
                buildid_proc.stderr.decode("utf-8", errors="surrogateescape"),
            )
        )
    buildid = buildid_proc.stdout

    # Prefer to install the debug info by BuildID since it does not require another
    # objcopy invocation and is more standard
    with open(metadata, "w") as f:
        if buildid := buildid[len(buildid) - 20 :].hex():
            json.dump({"buildid": buildid}, f)
        else:
            # Can't setup debuglink here as we don't reliably know the location the binary
            # will end up being placed under, which debuglink relies on, so opt to no-op
            # here and linking will ultimately be handled in the install feature.
            json.dump({}, f)


def cmd_metadata(args: argparse.Namespace) -> None:
    write_metadata(args.binary, args.objcopy, args.metadata)


def cmd_split_dir(args: argparse.Namespace) -> None:
    """Split all binaries in a directory, producing stripped/debuginfo/metadata output dirs."""
    input_dir = args.input_dir
    stripped_dir = args.stripped_dir
    debuginfo_dir = args.debuginfo_dir
    metadata_dir = args.metadata_dir

    stripped_dir.mkdir(parents=True, exist_ok=True)
    debuginfo_dir.mkdir(parents=True, exist_ok=True)
    metadata_dir.mkdir(parents=True, exist_ok=True)

    for filepath in sorted(input_dir.rglob("*")):
        if not filepath.is_file():
            continue

        relpath = filepath.relative_to(input_dir)

        stripped_path = stripped_dir / relpath
        stripped_path.parent.mkdir(parents=True, exist_ok=True)

        debuginfo_path = debuginfo_dir / relpath
        debuginfo_path.parent.mkdir(parents=True, exist_ok=True)

        metadata_path = metadata_dir / relpath.parent / (relpath.name + ".json")
        metadata_path.parent.mkdir(parents=True, exist_ok=True)

        strip_binary(filepath, args.objcopy, stripped_path, args.strip_all)
        extract_debuginfo(filepath, args.objcopy, debuginfo_path)
        write_metadata(filepath, args.objcopy, metadata_path)


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

    split_dir_parser = subparsers.add_parser("split-dir")
    split_dir_parser.add_argument("--objcopy", required=True)
    split_dir_parser.add_argument("--input-dir", required=True, type=Path)
    split_dir_parser.add_argument("--stripped-dir", required=True, type=Path)
    split_dir_parser.add_argument("--debuginfo-dir", required=True, type=Path)
    split_dir_parser.add_argument("--metadata-dir", required=True, type=Path)
    split_dir_parser.add_argument(
        "--strip-all",
        action="store_true",
        default=False,
    )
    split_dir_parser.set_defaults(func=cmd_split_dir)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
