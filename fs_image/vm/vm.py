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
import uuid
from contextlib import AsyncExitStack, asynccontextmanager, contextmanager
from dataclasses import dataclass
from itertools import chain
from pathlib import Path
from typing import AsyncContextManager, ContextManager, Iterable, Optional

from fs_image.vm.guest_agent import QemuError, QemuGuestAgent
from fs_image.vm.share import Share


logger = logging.getLogger(__name__)


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
    try:
        # QEMU BIOSes are a FB-specific resource
        with importlib.resources.path(
            "fs_image.vm", "qemu_bioses"
        ) as qemu_bioses:
            bios_dir = qemu_bioses
    except FileNotFoundError:
        bios_dir = None

    with importlib.resources.path(
        "fs_image.vm", "vmlinuz"
    ) as vmlinuz, importlib.resources.path(
        "fs_image.vm", "initrd"
    ) as initrd, importlib.resources.path(
        "fs_image.vm", "modules"
    ) as modules, importlib.resources.path(
        "fs_image.vm", "qemu"
    ) as qemu:
        yield KernelResources(
            vmlinuz=vmlinuz,
            initrd=initrd,
            modules=modules,
            qemu=qemu,
            qemu_bioses=bios_dir,
        )


async def __wait_for_boot(sockfile: os.PathLike) -> None:
    """
    The guest sends a READY message to this socket when it is ready to run
    a test. Block until that message is received.
    """
    # qemu might not have started up on the first couple connection attempts,
    # so wait until it binds to this socket path
    logger.debug("connecting to qemu guest notify socket")
    while True:
        try:
            conn_r, _ = await asyncio.open_unix_connection(sockfile)
            break
        except FileNotFoundError:
            await asyncio.sleep(0.1)

    logger.debug("waiting for guest boot event")
    await conn_r.readline()
    logger.debug("received guest boot event")


@asynccontextmanager
async def __kernel_vm_with_stack(
    stack: AsyncExitStack,
    image: Path,
    fbcode: Optional[Path] = None,
    verbose: bool = False,
    interactive: bool = False,
    shares: Optional[Iterable[Share]] = None,
    dry_run: Optional[bool] = False,
    up_timeout: Optional[int] = 2 * 60,
    ncpus: Optional[int] = 1,
):
    # we don't actually want to create files for the socket paths
    guest_agent_sockfile = os.path.join(
        tempfile.gettempdir(), "vmtest_guest_agent" + uuid.uuid4().hex + ".sock"
    )
    notify_sockfile = os.path.join(
        tempfile.gettempdir(), "vmtest_notify_" + uuid.uuid4().hex + ".sock"
    )

    rwdevice = stack.enter_context(
        tempfile.NamedTemporaryFile(
            prefix="vm_",
            suffix="_rw.img",
            # If available, create this temporary disk image in a temporary
            # directory that we know will be on disk, instead of /tmp which may
            # be a space-constrained tmpfs whichcan cause sporadic failures
            # depending on how much VMs decide to write to the root partition
            # multiplied by however many VMs are running concurrently. If
            # DISK_TEMP is not set, Python will follow the normal mechanism to
            # determine where to create this file as described in:
            # https://docs.python.org/3/library/tempfile.html#tempfile.gettempdir
            dir=os.getenv("DISK_TEMP"),
        )
    )
    # TODO: should this size be configurable (or is it possible to dynamically
    # grow)?
    rwdevice.truncate(1 * 1024 * 1024 * 1024)

    shares = shares or []
    # Mount directories that are specific to the Facebook
    try:
        from fs_image.facebook.vm.share_fbcode_runtime import (
            gen_fb_share_fbcode_runtime as _gen_fb_share_fbcode_runtime,
        )

        shares.extend(_gen_fb_share_fbcode_runtime())
    except ImportError:  # pragma: no cover
        pass

    if fbcode is not None:
        # also share fbcode at the same mount point from the host so that
        # absolute symlinks in fbcode work when in @mode/dev
        shares += [Share(fbcode)]

    with kernel_resources() as kernel:
        args = [
            "-no-reboot",
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
            "-object",
            "rng-random,filename=/dev/urandom,id=rng0",
            "-device",
            "virtio-rng-pci,rng=rng0",
            "-net",
            "none",
            "-device",
            "virtio-serial",
            "-kernel",
            str(kernel.vmlinuz),
            "-initrd",
            str(kernel.initrd),
            "-append",
            (
                "console=ttyS0,115200"
                " root=/dev/vda"
                " noapic"
                " panic=-1"
                " cgroup_no_v1=all"
                " systemd.unified_cgroup_hierarchy=1"
                " rootflags=subvol=volume"
                " rw"
                " rd.emergency=poweroff"
                " rd.debug"
            ),
            "-drive",
            f"file={image},if=virtio,format=raw,readonly=on",
            "-drive",
            f"file={rwdevice.name},if=virtio,format=raw,readonly=off",
            # qemu-guest-agent socket/serial device pair
            "-chardev",
            f"socket,path={guest_agent_sockfile},server,nowait,id=qga0",
            "-device",
            "virtserialport,chardev=qga0,name=org.qemu.guest_agent.0",
            # socket/serial device pair (for use by __wait_for_boot)
            "-chardev",
            f"socket,path={notify_sockfile},id=notify,server",
            "-device",
            "virtserialport,chardev=notify,name=notify-host",
        ]

        # Only set directory for the BIOS if qemu_bioses are provided
        if kernel.qemu_bioses:
            args.extend(["-L", str(kernel.qemu_bioses)])

        if os.access("/dev/kvm", os.R_OK | os.W_OK):
            args += ["-enable-kvm"]
        else:
            logger.warning(
                "KVM not available - falling back to slower, emulated CPU: "
                + "see https://our.intern.facebook.com/intern/qa/5312/"
                + "how-do-i-enable-kvm-on-my-devvm"
            )

        # this modules directory is mounted by init.sh at boot, to avoid having
        # to install kernels in the root fs and avoid expensive copying of
        # ~400M worth of modules during boot
        shares += [
            Share(kernel.modules, mount_tag="kernel-modules", generator=False)
        ]

        export_share = stack.enter_context(Share.export_spec(shares))
        shares += [export_share]

        args += __qemu_share_args(shares)
        if dry_run:
            print(
                str(kernel.qemu) + " " + " ".join(shlex.quote(a) for a in args)
            )
            sys.exit(0)

        if interactive:
            proc = await asyncio.create_subprocess_exec(str(kernel.qemu), *args)
        elif verbose:
            # don't connect stdin if we are simply in verbose mode and not
            # interactive
            proc = await asyncio.create_subprocess_exec(
                str(kernel.qemu), *args, stdin=subprocess.PIPE
            )
        else:
            proc = await asyncio.create_subprocess_exec(
                str(kernel.qemu),
                *args,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
            )

        try:
            await asyncio.wait_for(
                __wait_for_boot(notify_sockfile), timeout=up_timeout
            )
        except asyncio.TimeoutError:
            proc.terminate()
            await proc.wait()
            raise QemuError(f"guest failed to boot before {up_timeout}s")

        try:
            yield QemuGuestAgent(guest_agent_sockfile, up_timeout)
        except QemuError as err:
            print(f"Qemu failed with error: {err}", flush=True, file=sys.stderr)
        finally:
            if interactive:
                logger.debug("waiting for interactive vm to shutdown")
                await proc.wait()
            if proc.returncode is None:
                logger.debug("killing guest")
                proc.terminate()
                await proc.wait()


@asynccontextmanager
async def kernel_vm(*args, **kwargs) -> AsyncContextManager[QemuGuestAgent]:
    async with AsyncExitStack() as stack:
        async with __kernel_vm_with_stack(*args, **kwargs, stack=stack) as vm:
            yield vm


def __qemu_share_args(shares: Iterable[Share]) -> Iterable[str]:
    return chain.from_iterable(
        (
            "-virtfs",
            (
                f"local,id=fs{i},path={share.path},security_model=none,"
                f"readonly,multidevs=remap,mount_tag={share.mount_tag}"
            ),
        )
        for i, share in enumerate(shares)
    )
