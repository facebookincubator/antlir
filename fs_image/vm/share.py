#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import ContextManager, Iterable


__next_tag_index = 0


def _next_tag() -> str:
    global __next_tag_index
    tag = f"fs{__next_tag_index}"
    __next_tag_index += 1
    return tag


# Representation of a filesystem device mounted and exposed to the guest (qemu)
# using a virtio-9p-device.
@dataclass(frozen=True)
class Share(object):
    path: os.PathLike
    mount_tag: str = field(default_factory=_next_tag)
    generator: bool = True

    @property
    def __mount_unit_name(self) -> str:
        return subprocess.run(
            ["systemd-escape", "--suffix=mount", "--path", self.path],
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()

    @property
    def __mount_unit(self) -> str:
        return f"""[Unit]
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What={self.mount_tag}
Where={str(self.path)}
Type=9p
Options=version=9p2000.L,posixacl,cache=loose,ro
"""

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
                unit_path = os.path.join(exportdir, share.__mount_unit_name)
                with open(unit_path, "w") as f:
                    f.write(share.__mount_unit)
            yield Share(exportdir, mount_tag="exports")
