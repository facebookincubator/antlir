#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import importlib.resources
import logging
import os
import shlex
import subprocess
import sys
import tempfile
from contextlib import asynccontextmanager, contextmanager
from dataclasses import dataclass
from itertools import chain
from pathlib import Path
from typing import AsyncContextManager, ContextManager, Iterable, Optional, Union

from fs_image.vm.guest_agent import QemuGuestAgent
from fs_image.vm.share import Share, process_shares


logger = logging.getLogger("vm")


@dataclass(frozen=True)
class KernelResources(object):
    # these paths vary per kernel
    vmlinuz: Path
    initrd: Path
    modules: Path
    # these are invariant and a part of //fs_image/vm:vm
    qemu: Path
    qemu_bioses: Path


@contextmanager
def kernel_resources() -> ContextManager[KernelResources]:
    with importlib.resources.path(
        "fs_image.vm", "vmlinuz"
    ) as vmlinuz, importlib.resources.path(
        "fs_image.vm", "initrd"
    ) as initrd, importlib.resources.path(
        "fs_image.vm", "modules"
    ) as modules, importlib.resources.path(
        "fs_image.vm", "qemu"
    ) as qemu, importlib.resources.path(
        "fs_image.vm", "qemu_bioses"
    ) as bios_dir:
        yield KernelResources(
            vmlinuz=vmlinuz,
            initrd=initrd,
            modules=modules,
            qemu=qemu,
            qemu_bioses=bios_dir,
        )


@asynccontextmanager
async def kernel_vm(
    image: Path,
    fbcode: Optional[Path] = None,
    verbose: bool = False,
    interactive: bool = False,
    shares: Optional[Iterable[Union[Share, Path]]] = None,
    dry_run: Optional[bool] = False,
    up_timeout: Optional[int] = 2 * 60,
    ncpus: Optional[int] = 1,
) -> AsyncContextManager[QemuGuestAgent]:
    # An image should always be provided; either by vmtest or run_vm
    assert image

    sockfile = tempfile.NamedTemporaryFile(
        prefix="qemu_guest_agent_",
        suffix=".sock",
        # qemu will delete this socket file
        delete=False,
    ).name

    # this ephemeral disk file will be deleted when it gets garbage collected,
    # which will happen after QEMU finishes, whether it succeeds or fails
    rwdevice = tempfile.NamedTemporaryFile(
        prefix="vm_",
        suffix="_rw.img",
        # If available, create this temporary disk image in a temporary
        # directory that we know will be on disk, instead of /tmp which may be
        # a space-constrained tmpfs whichcan cause sporadic failures depending
        # on how much VMs decide to write to the root partition multiplied by
        # however many VMs are running concurrently.
        # If DISK_TEMP is not set, Python will follow the normal mechanism to
        # determine where to create this file as described in:
        # https://docs.python.org/3/library/tempfile.html#tempfile.gettempdir
        dir=os.getenv("DISK_TEMP"),
    )
    # TODO: should this size be configurable (or is it possible to dynamically grow)?
    rwdevice.truncate(1 * 1024 * 1024 * 1024)

    shares = process_shares(shares)
    # Mount directories that are specific to the Facebook
    try:
        from fs_image.facebook.vm.share_fbcode_runtime import (
            gen_fb_share_fbcode_runtime as _gen_fb_share_fbcode_runtime,
        )

        shares.extend(_gen_fb_share_fbcode_runtime())
    except ImportError:  # pragma: no cover
        pass

    if fbcode is not None:
        # also share fbcode at the same mount point from the host
        # so that absolute symlinks in fbcode work when in @mode/dev
        shares += [
            Share(
                host_path=fbcode, mount_tag="fbcode", location=fbcode, agent_mount=True
            )
        ]
    with kernel_resources() as kernel:
        args = [
            "-display",
            "none",
            "-serial",
            "mon:stdio",
            "-cpu",
            "max",
            "-smp",
            str(ncpus),
            "-m",
            "4G",
            "-device",
            "virtio-rng-pci",
            "-net",
            "none",
            "-device",
            "virtio-serial",
            "-kernel",
            str(kernel.vmlinuz),
            "-initrd",
            str(kernel.initrd),
            "-append",
            "console=ttyS0,115200 root=/dev/vda noapic cgroup_no_v1=all systemd.unified_cgroup_hierarchy=1 rootflags=subvol=volume rw rd.emergency=poweroff rd.debug",
            "-drive",
            f"file={image},if=virtio,format=raw,readonly=on",
            "-drive",
            f"file={rwdevice.name},if=virtio,format=raw,readonly=off",
            "-chardev",
            f"socket,path={sockfile},server,nowait,id=qga0",
            "-device",
            "virtio-serial",
            "-device",
            "virtserialport,chardev=qga0,name=org.qemu.guest_agent.0",
            "-L",
            str(kernel.qemu_bioses),
        ]
        if os.access("/dev/kvm", os.R_OK | os.W_OK):
            args += ["-enable-kvm"]
        else:
            print(
                "KVM not available - falling back to slower, emulated CPU: "
                + "see https://our.intern.facebook.com/intern/qa/5312/how-do-i-enable-kvm-on-my-devvm",
                file=sys.stderr,
            )

        args += __qemu_share_args(shares)
        if dry_run:
            print(str(kernel.qemu) + " " + " ".join(shlex.quote(a) for a in args))
            sys.exit(0)

        if interactive:
            proc = await asyncio.create_subprocess_exec(str(kernel.qemu), *args)
        elif verbose:
            # don't connect stdin if we are simply in verbose mode and not interactive
            proc = await asyncio.create_subprocess_exec(
                str(kernel.qemu), *args, stdin=subprocess.PIPE
            )
        else:
            proc = await asyncio.create_subprocess_exec(
                str(kernel.qemu),
                *args,
                stdin=subprocess.PIPE,
                stdout=sys.stderr,
                stderr=subprocess.STDOUT,
            )

        ga = QemuGuestAgent(sockfile, connect_timeout=up_timeout)
        try:
            for share in [s for s in shares if s.agent_mount]:
                await ga.mount_share(tag=share.mount_tag, mountpoint=share.location)
            yield ga
        finally:
            if interactive:
                await proc.wait()
            if proc.returncode is None:
                proc.terminate()
                await proc.wait()


def __qemu_share_args(shares: Iterable[Share]) -> Iterable[str]:
    return chain.from_iterable(
        (
            "-virtfs",
            f"local,path={share.host_path},security_model=none,readonly,mount_tag={share.mount_tag}",
        )
        for share in shares
    )
