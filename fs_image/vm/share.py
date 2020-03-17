#!/usr/bin/env python3
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional, Union


# Representation of a filesystem device mounted and exposed to the guest (qemu)
# using a virtio-9p-device.
@dataclass(frozen=True)
class Share(object):
    host_path: Path
    mount_tag: str
    location: Optional[str] = None
    agent_mount: bool = True


# Utility method to convert an Iterable with a mix of `Path` and `Share`
# objects to contain solely `Share` objects.
def process_shares(
    shares: Optional[Iterable[Union[Share, Path]]] = None
) -> Iterable[Share]:
    if shares:
        shares = [
            Share(host_path=share, mount_tag=share.name, location=None)
            if isinstance(share, Path)
            else share
            for share in shares
        ]
    else:
        shares = []

    return shares
