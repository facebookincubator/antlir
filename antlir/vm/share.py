#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
from abc import ABC, abstractmethod
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import ContextManager, Iterable, Optional, Tuple, Union

from antlir.fs_utils import Path


__next_tag_index = 0
# the first two disks MUST be rootfs and scratch device
__next_drive_index = 2


def _next_tag() -> str:
    global __next_tag_index
    tag = f"fs{__next_tag_index}"
    __next_tag_index += 1
    return tag


def _next_drive() -> str:
    global __next_drive_index
    idx = __next_drive_index
    __next_drive_index += 1
    return "vd" + chr(idx + ord("a"))


class Share(ABC):
    @property
    @abstractmethod
    def generator(self) -> bool:
        """Should this share have a systemd mount unit generated for it"""
        pass

    @property
    @abstractmethod
    def mount_unit(self) -> Tuple[str, str]:
        """Return the name of the mount unit file, and its contents"""
        pass

    @property
    @abstractmethod
    def qemu_args(self) -> Iterable[str]:
        """QEMU cmdline args to attach this share"""
        pass

    @staticmethod
    def _systemd_escape_mount(path: str) -> str:
        return subprocess.run(
            ["systemd-escape", "--suffix=mount", "--path", path],
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()

    @staticmethod
    @contextmanager
    def export_spec(shares: Iterable["Share"]) -> ContextManager["Share"]:
        """share a meta-directory that contains all the mount tags and paths to
        mount them, which is then read early in boot by a systemd generator
        this cannot be performed with just the export tags, because encoding the
        full path would frequently make them too long to be valid 9p tags"""
        with tempfile.TemporaryDirectory() as exportdir:
            for share in shares:
                if not share.generator:
                    continue
                unit_name, unit_contents = share.mount_unit
                unit_path = os.path.join(exportdir, unit_name)
                with open(unit_path, "w") as f:
                    f.write(unit_contents)
            yield Plan9Export(exportdir, mount_tag="exports")


@dataclass(frozen=True)
class Plan9Export(Share):
    """9PFS share of a host directory to the guest"""

    path: Path
    mountpoint: Optional[Path] = None
    mount_tag: str = field(default_factory=_next_tag)
    generator: bool = True
    # This should be used in readonly mode unless absolutely necessary.
    readonly: bool = True

    def __post_init__(self):
        if not self.mountpoint:
            object.__setattr__(self, "mountpoint", self.path)

    @property
    def mount_unit(self) -> Tuple[str, str]:
        cache = "loose" if self.readonly else "none"
        ro_rw = "ro" if self.readonly else "rw"
        return (
            self._systemd_escape_mount(self.mountpoint),
            f"""[Unit]
Description=Mount {self.mount_tag} at {self.mountpoint}
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What={self.mount_tag}
Where={self.mountpoint}
Type=9p
Options=version=9p2000.L,posixacl,cache={cache},{ro_rw}
""",
        )

    @property
    def qemu_args(self) -> Iterable[str]:
        maybe_readonly = ",readonly" if self.readonly else ""
        return (
            "-virtfs",
            (
                f"local,path={self.path},security_model=none,"
                f"multidevs=remap,mount_tag={self.mount_tag}{maybe_readonly}"
            ),
        )


@dataclass(frozen=True)
class BtrfsDisk(Share):
    """Share a btrfs image file as a virtio disk."""

    path: os.PathLike
    mountpoint: str
    dev: str = field(default_factory=_next_drive)
    generator: bool = True
    subvol: str = "volume"
    readonly: bool = True

    @property
    def mount_unit(self) -> str:
        ro_rw = "ro" if self.readonly else "rw"
        return (
            self._systemd_escape_mount(self.mountpoint),
            f"""[Unit]
Description=Mount {self.dev} ({self.path} from host) at {self.mountpoint}
Before=local-fs.target

[Mount]
What=/dev/{self.dev}
Where={str(self.mountpoint)}
Type=btrfs
Options=subvol={self.subvol},{ro_rw}
""",
        )

    @property
    def qemu_args(self) -> Iterable[str]:
        readonly = "on" if self.readonly else "off"
        return (
            "-drive",
            f"if=virtio,format=raw,file={self.path},readonly={readonly}",
        )
