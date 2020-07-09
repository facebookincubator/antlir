#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is a poor man's port of set_up_volume.sh to allow `image_package` to
emit btrfs loopbacks.  In ~1 weeks' time, this will be replaced by a
better-tested, more robust, and more coherent framework for handling images
and loopbacks.
"""
import logging
import os
import subprocess
import sys
import tempfile
from typing import Optional

from .common import byteme, get_file_logger, run_stdout_to_err
from .unshare import Unshare, nsenter_as_root, nsenter_as_user


log = get_file_logger(__file__)
MiB = 2 ** 20
# Otherwise, `mkfs.btrfs` fails with:
#   ERROR: minimum size for each btrfs device is 114294784
MIN_CREATE_BYTES = 109 * MiB
# The smallest size, to which btrfs will GROW a tiny filesystem. For
# lower values, `btrfs resize` prints:
#   ERROR: unable to resize '_foo/volume': Invalid argument
# MIN_GROW_BYTES = 175 * MiB
#
# When a filesystem's `min-dev-size` is small, `btrfs resize` below this
# limit will fail to shrink with `Invalid argument`.
MIN_SHRINK_BYTES = 256 * MiB


def _round_to_loop_block_size(num_bytes: int, log_level: int) -> int:
    """
    Avoid T24578982: btrfs soft lockup: `losetup --set-capacity /dev/loopN`
    wrongly sets block size to 1024 when backing file size is 4096-odd.

    Future: maybe we shouldn't hardcode 4096, but instead query:
        blockdev --getbsz /dev/loopSOMETHING
    """
    block_size = 4096
    rounded = num_bytes + (block_size - (num_bytes % block_size)) % block_size
    if num_bytes != rounded:
        log.log(
            log_level,
            f"Rounded image size {num_bytes} up to {rounded} to avoid kernel "
            "bug.",
        )
    return rounded


def _create_or_resize_image_file(
    path: bytes, at_least_bytes: int, log_level: int = logging.INFO
):
    """
    Be sure to call `btrfs filesystem resize` and `losetup --set-capacity`
    in the appropriate order.
    """
    rounded_bytes = _round_to_loop_block_size(at_least_bytes, log_level)
    run_stdout_to_err(["truncate", "-s", str(rounded_bytes), path], check=True)


def _fix_up_fs_size(size_bytes: int, min_usable_fs_size: int) -> int:
    if size_bytes < min_usable_fs_size:
        log.warning(
            f"btrfs cannot use a size of {size_bytes} < {min_usable_fs_size} "
            "bytes, will use the larger size"
        )
        return min_usable_fs_size
    return size_bytes


def _format_image_file(path: bytes, size_bytes: int) -> int:
    "Returns the actual filesystem size, which may have been increased."
    size_bytes = _fix_up_fs_size(size_bytes, MIN_CREATE_BYTES)
    log.info(f"Formatting btrfs {size_bytes}-byte FS at {path}")
    _create_or_resize_image_file(path, size_bytes)
    # Note that this can fail with 'cannot check mount status' if the
    # host is in a bad state:
    #  - a file backing a loop device got deleted, or
    #  - multiple filesystems with the same UUID got mounted as a loop
    #    device, breaking the metadata for the affected loop device (this
    #    latter issue is a kernel bug).
    # We don't check for this error case since there's nothing we can do to
    # remediate it.
    # The default profile for btrfs filesystem is the DUP. The man page says:
    # > The mkfs utility will let the user create a filesystem with profiles
    # > that write the logical blocks to 2 physical locations.
    # Switching to the SINGLE profile (below) saves a lot of space (30-40% as
    # reported by `btrfs inspect-internal min-dev-size`), and loses some
    # redundancy on rotational hard drives. Long history of using `-m single`
    # never showed any issues with such lesser redundancy.
    run_stdout_to_err(["mkfs.btrfs", "-m", "single", path], check=True)
    return size_bytes


def _mount_image_file(
    unshare: Optional[Unshare], file_path: bytes, mount_path: bytes
) -> bytes:
    log.info(f"Mounting btrfs {file_path} at {mount_path}")
    # Explicitly set filesystem type to detect shenanigans.
    run_stdout_to_err(
        nsenter_as_root(
            unshare,
            "mount",
            "-t",
            "btrfs",
            "-o",
            "loop,discard,nobarrier,compress=zstd",
            file_path,
            mount_path,
        ),
        check=True,
    )
    loop_dev = subprocess.check_output(
        nsenter_as_user(
            unshare, "findmnt", "--noheadings", "--output", "SOURCE", mount_path
        )
    ).rstrip(b"\n")
    # This increases the chances that --direct-io=on will succeed, since one
    # of the common failure modes is that the loopback's sector size is NOT
    # a multiple of the sector size of the underlying device (the devices
    # we've seen in production have sector sizes of 512, 1024, or 4096).
    if (
        run_stdout_to_err(
            ["sudo", "losetup", "--sector-size=4096", loop_dev]
        ).returncode
        != 0
    ):
        log.error(
            f"Failed to set --sector-size=4096 for {loop_dev}, setting "
            "direct IO is more likely to fail."
        )
    # This helps perf and avoids doubling our usage of buffer cache.
    # Also, when the image is on tmpfs, setting direct IO fails.
    if (
        run_stdout_to_err(
            ["sudo", "losetup", "--direct-io=on", loop_dev]
        ).returncode
        != 0
    ):
        log.error(
            f"Could not enable --direct-io for {loop_dev}, expect worse "
            "performance."
        )
    return loop_dev


def _minimize_image_size(
    *,
    unshare: Optional[Unshare],
    cur_size: int,
    image_path: bytes,
    mount_path: bytes,
    loop_dev: bytes,
) -> int:
    "Returns the new filesystem size."
    min_size_out = subprocess.check_output(
        nsenter_as_root(
            unshare, "btrfs", "inspect-internal", "min-dev-size", mount_path
        )
    ).split(b" ")
    assert min_size_out[1] == b"bytes"
    min_size = _fix_up_fs_size(int(min_size_out[0]), MIN_SHRINK_BYTES)
    if min_size >= cur_size:
        log.info(
            f"Nothing to do: the minimum resize limit {min_size} is no less "
            f"than the current filesystem size of {cur_size} bytes."
        )
        return cur_size
    log.info(f"Shrinking {image_path} to the btrfs minimum, {min_size} bytes")
    run_stdout_to_err(
        nsenter_as_root(
            unshare, "btrfs", "filesystem", "resize", str(min_size), mount_path
        ),
        check=True,
    )
    fs_bytes = int(
        subprocess.check_output(
            nsenter_as_user(
                unshare,
                "findmnt",
                "--bytes",
                "--noheadings",
                "--output",
                "SIZE",
                mount_path,
            )
        )
    )
    # Log an error on size rounding since this is not expected to need it.
    _create_or_resize_image_file(image_path, fs_bytes, log_level=logging.ERROR)
    run_stdout_to_err(
        ["sudo", "losetup", "--set-capacity", loop_dev], check=True
    )
    return min_size


class LoopbackVolume:
    def __init__(
        self, unshare: Optional[Unshare], image_path: bytes, size_bytes: int
    ):
        self._unshare = unshare
        self._temp_dir_ctx = tempfile.TemporaryDirectory()  # noqa: P201
        self._size_bytes = size_bytes
        self._image_path = byteme(os.path.abspath(image_path))
        self._temp_dir: Optional[bytes] = None
        self._mount_dir: Optional[bytes] = None

    def __enter__(self) -> "LoopbackVolume":
        self._temp_dir = byteme(os.path.abspath(self._temp_dir_ctx.__enter__()))
        try:
            self._size_bytes = _format_image_file(
                self._image_path, self._size_bytes
            )
            self._mount_dir = os.path.join(self._temp_dir, b"volume")
            os.mkdir(self._mount_dir)
            self._loop_dev = _mount_image_file(
                self._unshare, self._image_path, self._mount_dir
            )
        except BaseException:
            self.__exit__(*sys.exc_info())
            raise
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        "This only suppresses exceptions if TemporaryDirectory.__exit__ does."
        if self._mount_dir:
            # If this throws, we won't be able to clean up `_mount_dir`, so
            # let the error fly.  If the loopback is inside an Unshare
            # object, the mount itself will eventually get cleaned up, but
            # we don't have ownership to trigger Unshare cleanup, and in any
            # case, that kind of clean-up is asynchronous, and would be
            # tricky to await properly.
            #
            # NB: It's possible to use tmpfs and namespaces to guarantee
            # cleanup, but it's just an empty directory in `/tmp`, so it's
            # really not worth the complexity.
            self.unmount_if_mounted()
        return self._temp_dir_ctx.__exit__(exc_type, exc_val, exc_tb)

    def unmount_if_mounted(self):
        if self._mount_dir:
            # Nothing might have been mounted, ignore exit code
            run_stdout_to_err(
                nsenter_as_root(self._unshare, "umount", self._mount_dir)
            )

    def dir(self) -> bytes:
        return self._mount_dir

    def minimize_size(self) -> int:
        "Returns the new image size."
        self._size_bytes = _minimize_image_size(
            unshare=self._unshare,
            cur_size=self._size_bytes,
            image_path=self._image_path,
            mount_path=self._mount_dir,
            loop_dev=self._loop_dev,
        )
        return self._size_bytes

    def get_size(self) -> int:
        return self._size_bytes
