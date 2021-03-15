#!/usr/bin/python3

import argparse
import os
import pwd
import socket
import subprocess
import sys
import uuid
from typing import List

from antlir.config import repo_config 
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.fs_utils import Path, generate_work_dir
from antlir.find_built_subvol import find_built_subvol


def main(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--build-layer",
        required=True,
        type=Path,
        help="An `image.layer` output path to use for building"
             "(`buck targets --show-output <TARGET>`)",
    )
    parser.add_argument(
        "--mode",
        required=True,
        type=str,
        help="The tool to invoke",
    )
    hostname =  os.getenv("ANTLIR_BUILD_HOSTNAME", uuid.uuid4().hex)

    opts, args = parser.parse_known_args(argv[1:])

    if not (socket.gethostname() == hostname):
        config = repo_config()
        work_dir = generate_work_dir()

        run_nspawn(
            new_nspawn_opts(
                hostname=hostname,
                layer=find_built_subvol(opts.build_layer),
                bind_repo_ro=True,
                bindmount_rw=[(config.repo_root, work_dir)],
                cmd=[
                    "/bin/bash",
                    "-cx",
                    f"cd {work_dir} && "
                    f"/usr/bin/{opts.mode} " +' '.join(["'"+arg+"'" for arg in args])
                ],
                setenv=[f"ANTLIR_BUILD_HOSTNAME={hostname}"],
                user=pwd.getpwuid(os.getuid()),
            ),
            PopenArgs(
                stdout=sys.stdout,
            ),
        )

    # This means the tool is already running inside the build layer
    # and has be re-invoked.
    else:
        subprocess.run([f"/usr/bin/{opts.mode}"] + args, check=True)

if __name__ == '__main__':
    main(sys.argv)
