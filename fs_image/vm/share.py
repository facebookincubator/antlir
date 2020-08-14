#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import tempfile
from dataclasses import dataclass, field
from typing import Iterable, Tuple


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

    @staticmethod
    def export_spec(
        shares: Iterable["Share"],
    ) -> Tuple[tempfile.TemporaryDirectory, "Share"]:
        """share a meta-directory that contains all the mount tags and paths to
        mount them, which is then read early in boot by a systemd generator
        this cannot be performed with just the export tags, because encoding the
        full path would frequently make them too long to be valid 9p tags"""
        exportdir = tempfile.TemporaryDirectory()  # noqa: P201,
        # this is released by the calling function, and will be cleaned up in
        # D22879966 with ExitStack
        with open(os.path.join(exportdir.name, "exports"), "w") as f:
            for share in shares:
                if not share.generator:
                    continue
                f.write(f"{share.mount_tag} {str(share.path)}\n")
        return exportdir, Share(exportdir.name, mount_tag="exports")
