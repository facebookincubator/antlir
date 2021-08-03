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
import os
import subprocess
import sys
from typing import Iterable, Optional

from .common import get_logger, kernel_version, run_stdout_to_err
from .fs_utils import Path, temp_dir
from .unshare import Unshare, nsenter_as_root, nsenter_as_user

log = get_logger()
KiB = 2 ** 10
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


class LoopbackVolume:
    def __init__(
        self,
        unshare: Optional[Unshare],
        image_path: Path,
        fs_type: str,
        # pyre-fixme[9]: mount_options has type `Iterable[str]`; used as `None`.
        mount_options: Iterable[str] = None,
    ):
        self._unshare = unshare
        self._temp_dir_ctx = temp_dir()
        self._image_path = Path(image_path).abspath()
        self._fs_type = fs_type
        self._mount_dir: Optional[Path] = None
        self._mount_options = mount_options or None
        self._temp_dir: Optional[Path] = None

    def __enter__(self) -> "LoopbackVolume":
        self._temp_dir = self._temp_dir_ctx.__enter__().abspath()
        try:
            # pyre-fixme[58]: `/` is not supported for operand types
            #  `Optional[Path]` and `bytes`.
            self._mount_dir = self._temp_dir / b"volume"
            # pyre-fixme[6]: Expected `Union[os.PathLike[bytes],
            # os.PathLike[str], bytes, str]` for 1st param but got
            # `Optional[Path]`.
            os.mkdir(self._mount_dir)
            # pyre-fixme[16]: `LoopbackVolume` has no attribute `_loop_dev`.
            self._loop_dev = self.mount()

        except BaseException:  # pragma: nocover
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

    def mount(self) -> Path:
        mount_opts = "loop,discard,nobarrier"
        if self._mount_options:
            mount_opts += ",{}".format(",".join(self._mount_options))

        log.info(
            f"Mounting {self._fs_type} {self._image_path} at {self._mount_dir} "
            f"with {mount_opts}"
        )
        # Explicitly set filesystem type to detect shenanigans.
        run_stdout_to_err(
            nsenter_as_root(
                self._unshare,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 2nd param but got `str`.
                "mount",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 3rd param but got `str`.
                "-t",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 4th param but got `str`.
                self._fs_type,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 5th param but got `str`.
                "-o",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 6th param but got `str`.
                mount_opts,
                self._image_path,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 8th param but got `Optional[Path]`.
                self._mount_dir,
            ),
            check=True,
        )

        loop_dev = subprocess.check_output(
            nsenter_as_user(
                self._unshare,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 2nd param but got `str`.
                "findmnt",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 3rd param but got `str`.
                "--noheadings",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 4th param but got `str`.
                "--output",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 5th param but got `str`.
                "SOURCE",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 6th param but got `Optional[Path]`.
                self._mount_dir,
            )
        ).rstrip(b"\n")

        # This increases the chances that --direct-io=on will succeed, since one
        # of the common failure modes is that the loopback's sector size is NOT
        # a multiple of the sector size of the underlying device (the devices
        # we've seen in production have sector sizes of 512, 1024, or 4096).
        if (
            run_stdout_to_err(
                # pyre-fixme[6]: Expected `Iterable[Variable[typing.AnyStr <:
                #  [str, bytes]]]` for 1st param but got
                #  `List[typing.Union[bytes, str]]`.
                ["sudo", "losetup", "--sector-size=4096", loop_dev]
            ).returncode
            != 0
        ):  # pragma: nocover
            log.error(
                f"Failed to set --sector-size=4096 for {loop_dev}, setting "
                "direct IO is more likely to fail."
            )
        # This helps perf and avoids doubling our usage of buffer cache.
        # Also, when the image is on tmpfs, setting direct IO fails.
        if (
            run_stdout_to_err(
                # pyre-fixme[6]: Expected `Iterable[Variable[typing.AnyStr <:
                #  [str, bytes]]]` for 1st param but got
                #  `List[typing.Union[bytes, str]]`.
                ["sudo", "losetup", "--direct-io=on", loop_dev]
            ).returncode
            != 0
        ):  # pragma: nocover
            log.error(
                f"Could not enable --direct-io for {loop_dev}, expect worse "
                "performance."
            )
        # pyre-fixme[7]: Expected `Path` but got `bytes`.
        return loop_dev

    def unmount_if_mounted(self):
        if self._mount_dir:
            # Nothing might have been mounted, ignore exit code
            run_stdout_to_err(
                nsenter_as_root(self._unshare, "umount", self._mount_dir)
            )

    def dir(self) -> Path:
        # pyre-fixme[7]: Expected `Path` but got `Optional[Path]`.
        return self._mount_dir


def btrfs_compress_mount_opts():
    # kernel versions pre-5.1 did not support compression level tuning
    return "compress=zstd" if kernel_version() < (5, 1) else "compress=zstd:19"


class BtrfsLoopbackVolume(LoopbackVolume):
    def __init__(self, size_bytes: int, **kwargs):
        if size_bytes < MIN_CREATE_BYTES:
            raise AttributeError(
                f"A btrfs loopback must be at least {MIN_CREATE_BYTES} bytes. "
                f"requested size: {size_bytes}"
            )

        self._size_bytes = size_bytes

        super().__init__(
            mount_options=[btrfs_compress_mount_opts()],
            fs_type="btrfs",
            **kwargs,
        )

    def __enter__(self) -> "BtrfsLoopbackVolume":
        try:
            self._format()
        except BaseException:  # pragma: nocover
            self.__exit__(*sys.exc_info())
            raise

        # pyre-fixme[7]: Expected `BtrfsLoopbackVolume` but got
        # `LoopbackVolume`.
        return super().__enter__()

    def _create_or_resize_image_file(self, size_bytes: int):
        """
        If this is resizing an existing loopback that is mounted, then
        be sure to call `btrfs filesystem resize` and `losetup --set-capacity`
        in the appropriate order.
        """

        # Avoid an old kernel bug that is fixed since 4.16:
        # btrfs soft lockup: `losetup --set-capacity /dev/loopN`
        # wrongly sets block size to 1024 when backing file size is 4096-odd.
        #
        # Future: maybe we shouldn't hardcode 4096, but instead query:
        # blockdev --getbsz /dev/loopSOMETHING
        if kernel_version() < (4, 16):

            block_size = 4096
            rounded = (
                size_bytes
                + (block_size - (size_bytes % block_size)) % block_size
            )
            if size_bytes != rounded:
                log.warning(
                    f"Rounded image size {size_bytes} up to {rounded} to avoid "
                    "kernel bug.",
                )
                size_bytes = rounded

        run_stdout_to_err(
            ["truncate", "-s", str(size_bytes), self._image_path], check=True
        )

        return size_bytes

    def receive(self, send: int) -> subprocess.CompletedProcess:
        """
        Receive a btrfs sendstream from the `send` fd
        """
        return run_stdout_to_err(
            # pyre-fixme[16]: `Optional` has no attribute `nsenter_as_root`.
            self._unshare.nsenter_as_root(
                "btrfs",
                "receive",
                self.dir(),
            ),
            stdin=send,
            stderr=subprocess.PIPE,
        )

    def _format(self):
        """
        Format the loopback image with a btrfs filesystem of size
        `self._size_bytes`
        """

        log.info(
            f"Formatting btrfs {self._size_bytes}-byte FS at {self._image_path}"
        )
        self._size_bytes = self._create_or_resize_image_file(self._size_bytes)
        # Note that this can fail with 'cannot check mount status' if the
        # host is in a bad state:
        #  - a file backing a loop device got deleted, or
        #  - multiple filesystems with the same UUID got mounted as a loop
        #    device, breaking the metadata for the affected loop device (this
        #    latter issue is a kernel bug).
        # We don't check for this error case since there's nothing we can do to
        # remediate it.
        # The default profile for btrfs filesystem is the DUP. The man page
        # says:
        # > The mkfs utility will let the user create a filesystem with profiles
        # > that write the logical blocks to 2 physical locations.
        # Switching to the SINGLE profile (below) saves a lot of space (30-40%)
        # as reported by `btrfs inspect-internal min-dev-size`), and loses some
        # redundancy on rotational hard drives. Long history of using
        # `-m single` never showed any issues with such lesser redundancy.
        run_stdout_to_err(
            [
                "mkfs.btrfs",
                "--metadata",
                "single",
                self._image_path,
            ],
            check=True,
        )

    def minimize_size(self) -> int:
        """
        Minimizes the loopback as much as possibly by inspecting
        the btrfs internals and resizing the filesystem explicitly.

        Returns the new size of the loopback in bytes.
        """
        min_size_out = subprocess.check_output(
            nsenter_as_root(
                self._unshare,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 2nd param but got `str`.
                "btrfs",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 3rd param but got `str`.
                "inspect-internal",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 4th param but got `str`.
                "min-dev-size",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 5th param but got `Optional[Path]`.
                self._mount_dir,
            )
        ).split(b" ")
        assert min_size_out[1] == b"bytes"
        maybe_min_size_bytes = int(min_size_out[0])
        # Btrfs filesystems cannot be resized below a certain limit, if if we
        # have a smaller fs than the limit, we just use the limit.
        min_size_bytes = (
            maybe_min_size_bytes
            if maybe_min_size_bytes >= MIN_SHRINK_BYTES
            else MIN_SHRINK_BYTES
        )

        if min_size_bytes >= self._size_bytes:
            log.info(
                f"Nothing to do: the minimum resize limit {min_size_bytes} "
                "is no less than the current filesystem size of "
                f"{self._size_bytes} bytes."
            )
            return self._size_bytes

        log.info(
            f"Shrinking {self._image_path} to the btrfs minimum: "
            f"{min_size_bytes} bytes."
        )
        run_stdout_to_err(
            nsenter_as_root(
                self._unshare,
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 2nd param but got `str`.
                "btrfs",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 3rd param but got `str`.
                "filesystem",
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 4th param but got `str`.
                "resize",
                str(min_size_bytes),
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 6th param but got `Optional[Path]`.
                self._mount_dir,
            ),
            check=True,
        )

        fs_bytes = int(
            subprocess.check_output(
                nsenter_as_user(
                    self._unshare,
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <:
                    #  [str, bytes]]]` for 2nd param but got `str`.
                    "findmnt",
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                    #  bytes]]]` for 3rd param but got `str`.
                    "--bytes",
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                    #  bytes]]]` for 4th param but got `str`.
                    "--noheadings",
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                    #  bytes]]]` for 5th param but got `str`.
                    "--output",
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                    #  bytes]]]` for 6th param but got `str`.
                    "SIZE",
                    # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                    #  bytes]]]` for 7th param but got `Optional[Path]`.
                    self._mount_dir,
                )
            )
        )
        self._create_or_resize_image_file(fs_bytes)
        run_stdout_to_err(
            # pyre-fixme[16]: `BtrfsLoopbackVolume` has no attribute
            # `_loop_dev`.
            ["sudo", "losetup", "--set-capacity", self._loop_dev],
            check=True,
        )

        assert min_size_bytes == fs_bytes

        self._size_bytes = min_size_bytes
        return self._size_bytes

    def get_size(self) -> int:
        return self._size_bytes
