#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
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

    @staticmethod
    @contextmanager
    def export_spec(shares: Iterable["Share"]) -> ContextManager["Share"]:
        """share a meta-directory that contains all the mount tags and paths to
        mount them, which is then read early in boot by a systemd generator
        this cannot be performed with just the export tags, because encoding the
        full path would frequently make them too long to be valid 9p tags"""
        with tempfile.TemporaryDirectory() as exportdir:
            with open(os.path.join(exportdir, "exports"), "w") as f:
                for share in shares:
                    if not share.generator:
                        continue
                    f.write(f"{share.mount_tag} {str(share.path)}\n")
            yield Share(exportdir, mount_tag="exports")
