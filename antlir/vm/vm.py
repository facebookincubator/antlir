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

from antlir.artifacts_dir import find_repo_root
from antlir.config import load_repo_config
from antlir.vm.guest_agent import QemuError, QemuGuestAgent
from antlir.vm.share import BtrfsDisk, Plan9Export, Share


logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class KernelResources(object):
    # these paths vary per kernel
    vmlinuz: Path
    initrd: Path
    modules: Path
    # these are invariant and a part of //antlir/vm:vm
    qemu: Path
    qemu_bioses: Path


@contextmanager
def kernel_resources() -> ContextManager[KernelResources]:
    try:
        # QEMU BIOSes are a FB-specific resource
        with importlib.resources.path(
            __package__, "qemu_bioses"
        ) as qemu_bioses:
            bios_dir = qemu_bioses
    except FileNotFoundError:
        bios_dir = None

    with importlib.resources.path(
        __package__, "vmlinuz"
    ) as vmlinuz, importlib.resources.path(
        __package__, "initrd"
    ) as initrd, importlib.resources.path(
        __package__, "modules"
    ) as modules, importlib.resources.path(
        __package__, "qemu"
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
async def __vm_with_stack(
    stack: AsyncExitStack,
    image: Path,
    bind_repo_ro: bool = False,
    verbose: bool = False,
    interactive: bool = False,
    shares: Optional[Iterable[Share]] = None,
    dry_run: Optional[bool] = False,
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

    # Load the repo_config
    repo_config = load_repo_config()

    shares = shares or []
    # The two initial disks (readonly rootfs seed device and the rw scratch
    # device) are required to have these two disk identifiers for the initrd to
    # be able to mount them. In the future, it might be possible to remove this
    # requirement in a systemd-based initrd that is a little more intelligent,
    # but is very low-pri now
    shares = [
        BtrfsDisk(path=str(image), dev="vda", generator=False, mountpoint="/"),
        BtrfsDisk(
            path=rwdevice.name,
            dev="vdb",
            generator=False,
            readonly=False,
            mountpoint="/",
        ),
    ] + shares

    # Mount directories that are specific to the Facebook
    try:
        from antlir.facebook.vm.share_fbcode_runtime import (
            gen_fb_share_fbcode_runtime as _gen_fb_share_fbcode_runtime,
        )

        shares.extend(_gen_fb_share_fbcode_runtime())
    except ImportError:  # pragma: no cover
        pass

    if bind_repo_ro or repo_config.artifacts_require_repo:
        # Mount the code repository root at the same mount point from the host
        # so that the symlinks that buck constructs in @mode/dev work
        shares += [Plan9Export(find_repo_root())]

        # Also share any additionally configured host mounts needed
        # along with the repository root.
        for mount in repo_config.host_mounts_for_repo_artifacts:
            shares.append(Plan9Export(mount))

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
            Plan9Export(
                kernel.modules, mount_tag="kernel-modules", generator=False
            )
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

        await __wait_for_boot(notify_sockfile)

        try:
            yield QemuGuestAgent(guest_agent_sockfile)
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
async def vm(*args, **kwargs) -> AsyncContextManager[QemuGuestAgent]:
    async with AsyncExitStack() as stack:
        async with __vm_with_stack(*args, **kwargs, stack=stack) as vm:
            yield vm


def __qemu_share_args(shares: Iterable[Share]) -> Iterable[str]:
    # The ordering of arguments for BtrfsDisk type shares is highly
    # significant, as it determines the drive device id
    disks = [s for s in shares if isinstance(s, BtrfsDisk)]
    disks = sorted(disks, key=lambda d: d.dev)
    other = [s for s in shares if not isinstance(s, BtrfsDisk)]
    shares = chain(disks, other)
    return chain.from_iterable(share.qemu_args for share in shares)
