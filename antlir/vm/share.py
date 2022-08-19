#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
from abc import ABC, abstractmethod
from contextlib import AsyncExitStack, contextmanager
from dataclasses import dataclass, field
from typing import Generator, Iterable, List, Optional, Tuple

from antlir.common import get_logger
from antlir.fs_utils import Path, temp_dir
from antlir.vm.bzl.vm import disk_interface_t


log = get_logger()

__next_tag_index = 0
__next_drive_index = 1


def _next_tag() -> str:
    global __next_tag_index
    tag = f"fs{__next_tag_index}"
    __next_tag_index += 1
    return tag


# TODO: this assumes all disks are virtio-blk which is not accurate anymore
def _next_drive() -> str:
    global __next_drive_index
    idx = __next_drive_index
    __next_drive_index += 1
    return "vd" + chr(idx + ord("a"))


class Share(ABC):
    @property
    def generator(self) -> bool:  # pragma: no cover
        """Should this share have a systemd mount unit generated for it"""
        return False

    @property
    def mount_unit(
        self,
    ) -> Tuple[Optional[str], Optional[str]]:  # pragma: no cover
        """
        Return the name of the mount unit file, and its contents.
        This is only applicable if `self.generator == True`.
        """
        return (None, None)

    @property
    @abstractmethod
    def qemu_args(self) -> Iterable[str]:  # pragma: no cover
        """QEMU cmdline args to attach this share"""

    @staticmethod
    def _systemd_escape_mount(path: Path) -> str:
        return subprocess.run(
            ["systemd-escape", "--suffix=mount", "--path", path],
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()

    @staticmethod
    @contextmanager
    def export_spec(
        shares: Iterable["Share"],
    ) -> Generator["Share", None, None]:
        """share a meta-directory that contains all the mount tags and paths to
        mount them, which is then read early in boot by a systemd generator
        this cannot be performed with just the export tags, because encoding the
        full path would frequently make them too long to be valid 9p tags"""
        with temp_dir() as exportdir:
            for share in shares:
                if not share.generator:
                    continue
                unit_name, unit_contents = share.mount_unit
                assert (
                    unit_name and unit_contents
                ), f"Invalid mount unit for {share}"
                unit_path = exportdir / unit_name
                with unit_path.open(mode="w") as f:
                    f.write(unit_contents)

            yield Plan9Export(
                path=exportdir, mountpoint=exportdir, mount_tag="exports"
            )


@dataclass(frozen=True)
class Plan9Export(Share):
    """9PFS share of a host directory to the guest"""

    path: Path
    mountpoint: Optional[Path] = None
    mount_tag: str = field(default_factory=_next_tag)
    generator: bool = True
    # This should be used in readonly mode unless absolutely necessary.
    readonly: bool = True

    def __post_init__(self) -> None:
        assert (
            self.generator or self.mountpoint is None
        ), f"`mountpoint` can not be set if `generator` is false: {self}"
        assert (
            not self.generator or self.mountpoint is not None
        ), f"`mountpoint` is required if `generator` is true: {self}"

    @property
    def mount_unit(self) -> Tuple[str, str]:
        assert self.generator
        assert self.mountpoint is not None
        cache = "loose" if self.readonly else "none"
        ro_rw = "ro" if self.readonly else "rw"
        return (
            self._systemd_escape_mount(self.mountpoint),
            f"""[Unit]
Description=Mount {self.mount_tag} at {self.mountpoint!s}
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What={self.mount_tag}
Where={self.mountpoint!s}
Type=9p
Options=version=9p2000.L,posixacl,cache={cache},{ro_rw}
""",
        )

    @property
    def qemu_args(self) -> Iterable[str]:
        readonly = "on" if self.readonly else "off"
        return (
            "-virtfs",
            (
                f"local,path={self.path!s},security_model=none,"
                f"multidevs=remap,mount_tag={self.mount_tag},"
                f"readonly={readonly}"
            ),
        )


def _run_qemu_img(qemu_img: Path, args: List) -> None:
    cmd = [qemu_img, *args]
    log.debug(f"Running qemu-img: {cmd}")

    try:
        # Combine stdout and stderr.
        ret = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        )
        log.debug(f"qemu-img complete: {ret}")

    except subprocess.CalledProcessError as e:  # pragma: no cover
        log.error(
            "Failed to run qemu-img. "
            f'Command: "{cmd}"; '
            f"Return value: {e.returncode}; "
            f"Output:\n {e.output.decode('utf-8')}"
        )
        raise


def _tmp_qcow2_disk(
    qemu_img: Path,
    stack: AsyncExitStack,
    backing_file: Path,
    additional_scratch_mb: Optional[int],
) -> Path:
    """
    Create a qcow2 scratch disk using qemu-img.
    """
    disk = stack.enter_context(
        tempfile.NamedTemporaryFile(
            prefix="vm_",
            suffix="_rw.qcow2",
            # If available, create this temporary disk image in a temporary
            # directory that we know will be on disk, instead of /tmp which
            # may be a space-constrained tmpfs whichcan cause sporadic
            # failures depending on how much VMs decide to write to the
            # root partition multiplied by however many VMs are running
            # concurrently. If DISK_TEMP is not set, Python will follow the
            # normal mechanism to determine where to create this file as
            # described in:
            # https://docs.python.org/3/library/tempfile.html#tempfile.gettempdir
            dir=os.getenv("DISK_TEMP"),
        )
    )
    _run_qemu_img(
        qemu_img,
        [
            "create",
            "-f",  # format
            "qcow2",
            disk.name,
            "-F",  # backing format
            "raw",
            "-b",
            backing_file,
        ],
    )

    if additional_scratch_mb is not None:
        _run_qemu_img(
            qemu_img,
            [
                "resize",
                disk.name,
                f"+{additional_scratch_mb}M",
            ],
        )

    return Path(disk.name)


@dataclass(frozen=True)
class QCow2Disk(Share):
    """
    Share a btrfs filesystem to a Qemu instance as a
    qcow2 disk. Defaults to virtio-blk, but can also be attached with other
    emulated interfaces such as nvme
    """

    path: Path
    qemu_img: Path
    stack: AsyncExitStack
    interface: disk_interface_t
    subvol: str = "volume"
    additional_scratch_mb: Optional[int] = None
    cow_disk: Optional[Path] = None
    dev: str = field(default_factory=_next_drive)

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            "cow_disk",
            _tmp_qcow2_disk(
                qemu_img=self.qemu_img,
                stack=self.stack,
                backing_file=self.path,
                additional_scratch_mb=self.additional_scratch_mb,
            ),
        )

    @property
    def qemu_args(self) -> Iterable[str]:
        return (
            "--blockdev",
            (
                f"driver=qcow2,node-name={self.dev},"
                f"file.driver=file,file.filename={self.cow_disk!s},"
            ),
            "--device",
            f"{self.interface.value},drive={self.dev},serial={self.dev}",
        )

    @property
    def kernel_args(self) -> Iterable[str]:
        return (
            f"rootflags=subvol={self.subvol}",
            "rootfstype=btrfs",
        )
