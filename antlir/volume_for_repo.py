#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import getpass
import os
import subprocess
import sys

from antlir import btrfsutil
from antlir.errors import InfraError

from antlir.fs_utils import Path

try:
    from antlir.facebook.sudo_error import FB_SUDO_ERROR
except ImportError:  # pragma: no cover
    FB_SUDO_ERROR = ""


VOLUME_DIR = "volume"


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
    if volume_dir.exists():
        # This could happen if it was a loopback and it got unmounted for
        # some reason without having the dir be deleted
        if not btrfsutil.is_subvolume(volume_dir):
            try:
                os.rmdir(volume_dir)
            except OSError as e:
                raise InfraError(
                    f"{volume_dir} is not a subvolume, but we couldn't remove"
                    " it. Please ensure it's empty and remove manually"
                ) from e
        # This could happen if the loopback volume is still mounted - the id
        # will never be 5 if it's just a subvolume on the regular host fs
        if (
            btrfsutil.is_subvolume(volume_dir)
            and btrfsutil.subvolume_id(volume_dir) == 5
        ):
            try:
                subprocess.run(["sudo", "umount", volume_dir], check=True)
            except subprocess.CalledProcessError as e:
                raise InfraError(
                    f"{volume_dir} appears to be a mounted btrfs fs, but"
                    " unmounting it failed. Please kill whatever is using it"
                    " and unmount manually"
                ) from e
            try:
                os.rmdir(volume_dir)
            except OSError as e:
                raise InfraError(
                    f"{volume_dir} could not be removed. Please ensure it's"
                    " empty and remove manually"
                ) from e
    # Normal case of an image build on a fresh environment, the subvolume
    # does not exist yet
    try:
        btrfsutil.create_subvolume(volume_dir)
    except btrfsutil.BtrfsUtilError as e:  # pragma: no cover
        if e.errno != errno.EEXIST:
            raise
    # We prefer to have the volume owned by the repo user, instead of root:
    #  - The trusted repo user has to be able to access the built
    #    subvolumes, but nobody else should be able to (they might contain
    #    setuid binaries & similar).  Thus, subvols ought to have wrapper
    #    directories owned by the user, with mode 0700.
    #  - This reduces the number of places we have to `sudo` to create
    #    directories inside the subvolume.
    subprocess.check_call(["sudo", "chown", f"{os.getuid()}:{os.getgid()}", volume_dir])
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
