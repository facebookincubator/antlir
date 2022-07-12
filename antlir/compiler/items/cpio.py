#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd

from antlir.compiler.items.common import LayerOpts

from antlir.fs_utils import generate_work_dir, open_for_read_decompress, Path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol


def load_from_cpio(
    source: str,
    subvol: Subvol,
    layer_opts: LayerOpts,
    into_dir=None,
    force_root_ownership: bool = False,
) -> None:
    into_dir = into_dir or Path("")

    build_appliance = layer_opts.requires_build_appliance()
    work_dir = generate_work_dir()
    cpio_cmd = " ".join(
        [
            "bsdtar",
            "-x",
            f"--directory {(work_dir / into_dir).decode()}",
            # Preserve sparse files
            "-S",
            # The uid:gid doing the extraction is root:root, so by default
            # tar would try to restore the file ownership from the archive.
            # In some cases, we just want all the files to be root-owned.
            *(["--no-same-owner"] if force_root_ownership else []),
            "--file -",
        ]
    )
    # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
    with open_for_read_decompress(source) as tf:
        opts = new_nspawn_opts(
            # '0<&3' below redirects fd=3 to stdin, so 'tar ... -f -' will
            # read and unpack whatever we represent as fd=3. We pass `tf` as
            # fd=3 into container by 'forward_fd=...' below. See help
            # string in antlir/nspawn_in_subvol/args.py where
            # _parser_add_nspawn_opts() calls
            # parser.add_argument('--forward-fd')
            cmd=["sh", "-uec", f"{cpio_cmd} 0<&3"],
            layer=build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam("root"),
            forward_fd=[tf.fileno()],
            allow_mknod=True,
        )
        run_nspawn(opts, PopenArgs())
