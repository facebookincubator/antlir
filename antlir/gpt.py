#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""\
Given a list of filesystem images, create a GPT disk image with each
partition being one of given images accordingly.
"""
import argparse
import os
import pwd
import subprocess
from enum import Enum

from antlir.bzl.gpt import gpt_t
from antlir.cli import normalize_buck_path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn

from .common import init_logging
from .find_built_subvol import find_built_subvol
from .fs_utils import generate_work_dir, Path
from .subvol_utils import MiB


class SgdiskTypeCodes(Enum):
    """
    Current we only support these 3 types (ESP, BIOS_BOOT and Linux filesystem)
    see `sgdisk -L` for details
    """

    ESP = "ef00"
    BIOS_BOOT = "ef02"
    LINUX = "8300"


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--output-path",
        required=True,
        type=normalize_buck_path,
        help="Write the GPT disk image to this path -- must not exist",
    )
    parser.add_argument(
        "--gpt",
        type=gpt_t.parse_raw,
        help="A serialized gpt_t instance containing configuration "
        "details for the GPT.",
        required=True,
    )
    parser.add_argument(
        "--build-appliance",
        help="Build appliance layer to use when creating GPT image",
        required=True,
    )
    return Path.parse_args(parser, argv)


def make_gpt(argv) -> None:
    args = parse_args(argv)
    output_path = args.output_path
    assert not os.path.exists(output_path)
    build_appliance = find_built_subvol(args.build_appliance)
    partitions = args.gpt.table
    assert len(partitions) > 0

    # prepare cmd params
    gpt_image_dir, gpt_image_name = os.path.split(output_path)
    work_dir = generate_work_dir()
    image_path = work_dir / gpt_image_name
    # 2 is the minimal integer MiB overhead that
    # won't cause trouble in my experiments
    image_size_mb = 2
    sgdisk_opts = []
    if args.gpt.disk_guid:
        sgdisk_opts.append(f"--disk-guid={args.gpt.disk_guid}")
    for p in partitions:
        ops = []
        # set partition size
        p_size_mb = int(os.path.getsize(p.package.path.decode()) / MiB)
        ops.append(f"-n 0:0:+{p_size_mb}M")
        # set partition type
        p_type_code = (
            SgdiskTypeCodes.ESP
            if p.is_esp
            else SgdiskTypeCodes.BIOS_BOOT
            if p.is_bios_boot
            else SgdiskTypeCodes.LINUX
        )
        ops.append(f"-t 0:{p_type_code.value}")
        if p.name:
            # optionally set partition name
            ops.append(f"-c 0:{p.name}")
        sgdisk_opts.append(" ".join(ops))
        image_size_mb += p_size_mb

    cmd = [
        "/bin/bash",
        "-eux",
        "-o",
        "pipefail",
        "-c",
        # create empty image
        "/usr/bin/truncate -s {image_size_mb}M {image_path}" " >/dev/null; "
        # make partitions
        "/usr/sbin/sgdisk {sgdisk_opts} {image_path}" " >/dev/null; "
        # get partition offsets
        "/usr/sbin/partx -o START -g --raw {image_path}".format(
            image_size_mb=image_size_mb,
            image_path=image_path,
            sgdisk_opts=" ".join(sgdisk_opts),
        ),
    ]
    res, _ = run_nspawn(
        new_nspawn_opts(
            cmd=cmd,
            layer=build_appliance,
            bindmount_rw=[(gpt_image_dir, work_dir)],
            user=pwd.getpwuid(os.getuid()),
        ),
        PopenArgs(stdout=subprocess.PIPE),
    )
    part_offsets = res.stdout.decode().strip().split()
    assert len(part_offsets) == len(partitions)

    for i, p in enumerate(partitions):
        subprocess.run(
            [
                "dd",
                f"if={p.package.path.decode()}",
                f"of={output_path}",
                "status=progress",
                f"seek={part_offsets[i]}",
                "conv=sparse,notrunc",
            ]
        )


if __name__ == "__main__":  # pragma: no cover
    import sys

    init_logging()
    make_gpt(sys.argv[1:])
