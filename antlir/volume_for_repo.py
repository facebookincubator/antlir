#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import getpass
import os
import subprocess
import sys

from antlir.fs_utils import Path

try:
    from antlir.facebook.sudo_error import FB_SUDO_ERROR
except ImportError:  # pragma: no cover
    FB_SUDO_ERROR = ""


# Exposed for tests
IMAGE_FILE = "image.btrfs"
VOLUME_DIR = "volume"

# The size of loop device when it is created first time. Overriding this may
# introduce an issue: test coverage would depend on the sequence of tests.
# For example, if we set this high in `image_layer_utils.bzl`, and low here,
# tests would pass if a "normal" volume is created first, but fail if a "tmp"
# volume is the first volume created.
LOOP_SIZE = 1e11


def get_volume_for_current_repo(artifacts_dir: Path):
    """
    Multiple repos need to be able to concurrently build images on the same
    host.  The cleanest way to achieve such isolation is to supply each repo
    with its own volume, which will store the repo's image build outputs.

    Image-build tooling **must never** access paths in this volume without
    going through this function.  Otherwise, the volume will not get
    remounted correctly if the host containing the repo got rebooted.

    PRE-CONDITION: `artifacts_dir` exists and is writable by `root`.
    """
    if not artifacts_dir.exists():  # pragma: no cover
        raise RuntimeError(f"{artifacts_dir} must exist")

    volume_dir = artifacts_dir / VOLUME_DIR
    with Path.resource(__package__, "set_up_volume.sh", exe=True) as binary:
        subprocess.check_call(
            [
                # While Buck probably does not call this concurrently under
                # normal circumstances, the worst-case outcome is that we lose
                # or corrupt the whole buld cache, so add some locking to be on
                # the safe side.
                "flock",
                artifacts_dir / ".lock.set_up_volume.sh.never.rm.or.mv",
                "sudo",
                binary,
                str(int(LOOP_SIZE)),  # Accepts floats & ints
                artifacts_dir / IMAGE_FILE,
                volume_dir,
            ]
        )
    # We prefer to have the volume owned by the repo user, instead of root:
    #  - The trusted repo user has to be able to access the built
    #    subvolumes, but nobody else should be able to (they might contain
    #    setuid binaries & similar).  Thus, subvols ought to have wrapper
    #    directories owned by the user, with mode 0700.
    #  - This reduces the number of places we have to `sudo` to create
    #    directories inside the subvolume.
    subprocess.check_call(
        ["sudo", "chown", f"{os.getuid()}:{os.getgid()}", volume_dir]
    )
    return volume_dir


if __name__ == "__main__":  # pragma: no cover
    # this is the first entry point for a CI build, where we should fail with
    # an understandable message if sudo is unavailable
    try:
        subprocess.run(["sudo", "--non-interactive", "true"], check=True)
    except subprocess.CalledProcessError:
        print(
            "image builds require sudo. "
            + f"Ensure that {getpass.getuser()} is allowed to use sudo."
            + FB_SUDO_ERROR,
            file=sys.stderr,
        )
        sys.exit(1)

    args = [Path(sys.argv[1])]
    print(get_volume_for_current_repo(*args))
