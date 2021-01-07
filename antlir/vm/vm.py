#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import asyncio
import importlib.resources
import os
import shlex
import subprocess
import sys
import tempfile
import uuid
from contextlib import AsyncExitStack, asynccontextmanager, contextmanager
from dataclasses import dataclass
from itertools import chain
from typing import (
    AsyncContextManager,
    Iterable,
    Optional,
    List,
)

from antlir.common import init_logging, get_logger
from antlir.compiler.items.mount import mounts_from_image_meta
from antlir.config import load_repo_config
from antlir.fs_utils import Path
from antlir.shape import Shape
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.unshare import Namespace, Unshare
from antlir.vm.guest_agent import QemuError, QemuGuestAgent
from antlir.vm.share import BtrfsDisk, Plan9Export, Share
from antlir.vm.tap import VmTap
from antlir.vm.vm_opts_t import vm_opts_t


logger = get_logger()


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


class VMExecOpts(Shape):
    """
    This is the common set of arguments that can be passed to an `antlir.vm`
    cli.
    """

    # Bind the repository root into the VM
    bind_repo_ro: bool = True
    # Extra, undefined arguments that are passed on the cli
    extra: List[str] = []
    # VM Opts instance passed to the CLI
    opts: vm_opts_t
    # Enable debug logs
    debug: bool = False

    # Future:  Since we're using `Shape` for this, which uses pydantic.  I
    # think it is possible automagically construct this based on the field
    # defintions of the type itself.  This would remove the need for the
    # overloading in subclasses.  That is a bit more magic than I wanted
    # to add at the moment, so I'm holding off.
    @classmethod
    def setup_cli(cls, parser):
        """
        Add attributes defined on this type to the parser.

        Subclasses of `VMExecOpts` should overload this classmethod to provide
        their own arguments.  Subclass implementors should take care to call
        `super(<SubClassType>, cls).setup_cli(parser)` to make sure that these
        base class args are added.
        """
        parser.add_argument(
            "--bind-repo-ro",
            action="store_true",
            default=True,
            help="Makes a read-only bind-mount of the current Buck project "
            "into the vm at the same location as it is on the host. This is "
            "needed to run binaries that are built to be run in-place and for "
            "binaries that make assumptions about files being available "
            "relative to the binary.",
        )

        parser.add_argument(
            "--opts",
            type=vm_opts_t.parse_raw,
            help="Path to a serialized vm_opts_t instance containing "
            "configuration details for the vm.",
            required=True,
        )

        parser.add_argument(
            "--debug",
            action="store_true",
            default=True,
            help="Show debug logs",
        )

    @classmethod
    def parse_cli(cls, argv) -> "VMExecOpts":
        """
        Construct a CLI parser, parse the arguments, and return a constructed
        instance from those arguments of type `cls`.
        """

        parser = argparse.ArgumentParser(
            description=__doc__,
            formatter_class=argparse.RawDescriptionHelpFormatter,
        )

        cls.setup_cli(parser)

        args, extra = parser.parse_known_args(argv)

        init_logging(debug=args.debug)

        if extra:
            logger.debug(f"Got extra args: {extra} from {argv}")
            args.extra = extra

        logger.debug(
            f"Creating instance of {cls} for VM execution args using: {args}"
        )
        return cls(**args.__dict__)


@asynccontextmanager
async def __vm_with_stack(
    stack: AsyncExitStack,
    opts: vm_opts_t,
    bind_repo_ro: bool = False,
    verbose: bool = False,
    interactive: bool = False,
    shares: Optional[Iterable[Share]] = None,
):
    # we don't actually want to create files for the socket paths
    guest_agent_sockfile = os.path.join(
        tempfile.gettempdir(), "vmtest_guest_agent" + uuid.uuid4().hex + ".sock"
    )
    notify_sockfile = os.path.join(
        tempfile.gettempdir(), "vmtest_notify_" + uuid.uuid4().hex + ".sock"
    )

    # Set defaults
    shares = shares or []

    # Load the repo_config
    # Note that we currently rely on the assumption that the binary that ends
    # up executing this code (//antlir/vm:vmtest or //antlir/vm:run) is being
    # executed while the cwd is within the repo path.  This might *not* always
    # be the case, but given the nature of the fact that these are invoked via
    # either `buck run` or `buck test` , and buck requires a working repo to
    # function, this is a reasonable assumption.  Note that we need this here
    # in the first place because we test that we can run a test binary inside
    # a VM via another test binary *outside* the VM and this kind of embedding
    # can cause the sys.argv[0] of the executing binary to live outside
    # of the repo. (See the //antlir/vm/tests:kernel_panic_test)
    repo_config = load_repo_config(path_in_repo=Path(os.getcwd()))

    # Process all the mounts from the root image we are using
    mounts = mounts_from_image_meta(opts.rootfs_image.path)

    for mount in mounts:
        if mount.build_source.type == "host":
            shares.append(
                Plan9Export(
                    path=mount.build_source.source,
                    mountpoint=Path("/") / mount.mountpoint,
                    mount_tag=mount.build_source.source.replace("/", "-")[1:],
                )
            )
        else:
            logger.warn(
                f"non-host mount found: {mount}. "
                "`antlir.vm` does not yet support "
                "non-host mounts"
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

    # The two initial disks (readonly rootfs seed device and the rw scratch
    # device) are required to have these two disk identifiers for the initrd to
    # be able to mount them. In the future, it might be possible to remove this
    # requirement in a systemd-based initrd that is a little more intelligent,
    # but is very low-pri now
    shares.extend(
        [
            BtrfsDisk(
                path=str(opts.rootfs_image.path),
                dev="vda",
                generator=False,
                mountpoint="/",
            ),
            BtrfsDisk(
                path=rwdevice.name,
                dev="vdb",
                generator=False,
                readonly=False,
                mountpoint="/",
            ),
        ]
    )

    if bind_repo_ro or repo_config.artifacts_require_repo:
        # Mount the code repository root at the same mount point from the host
        # so that the symlinks that buck constructs in @mode/dev work
        shares.append(Plan9Export(repo_config.repo_root))

        # Also share any additionally configured host mounts needed
        # along with the repository root.
        for mount in repo_config.host_mounts_for_repo_artifacts:
            shares.append(Plan9Export(mount))

    ns = stack.enter_context(Unshare([Namespace.NETWORK, Namespace.PID]))
    tapdev = VmTap(netns=ns, uid=os.getuid(), gid=os.getgid())
    args = [
        "-no-reboot",
        "-display",
        "none",
        "-serial",
        "mon:stdio",
        "-cpu",
        "max",
        "-smp",
        str(opts.cpus),
        "-m",
        "{}M".format(str(opts.mem_mb)),
        "-object",
        "rng-random,filename=/dev/urandom,id=rng0",
        "-device",
        "virtio-rng-pci,rng=rng0",
        "-device",
        "virtio-serial",
        "-kernel",
        str(opts.kernel.artifacts.vmlinuz.path),
        "-initrd",
        str(opts.initrd.path),
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
    ] + list(tapdev.qemu_args)

    # The bios to boot the emulator with
    args.extend(["-bios", str(opts.bios.path)])

    # Set the path for loading additional roms
    args.extend(["-L", str(opts.emulator_roms_dir.path)])

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
    if opts.kernel.artifacts.modules is not None:
        shares += [
            Plan9Export(
                opts.kernel.artifacts.modules.subvol.path(),
                mount_tag="kernel-modules",
                generator=False,
            )
        ]

    export_share = stack.enter_context(Share.export_spec(shares))
    shares += [export_share]

    args += __qemu_share_args(shares)

    qemu_cmd = ns.nsenter_as_user(str(opts.emulator.path), *args)

    if interactive:
        proc = await asyncio.create_subprocess_exec(*qemu_cmd)
    elif verbose:
        # don't connect stdin if we are simply in verbose mode and not
        # interactive
        proc = await asyncio.create_subprocess_exec(
            *qemu_cmd,
            stdin=subprocess.PIPE,
        )
    else:
        proc = await asyncio.create_subprocess_exec(
            *qemu_cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

    await __wait_for_boot(notify_sockfile)

    logger.debug(f"guest link-local ipv6: {tapdev.guest_ipv6_ll}")

    try:
        yield QemuGuestAgent(guest_agent_sockfile)
    except QemuError as err:
        print(f"Qemu failed with error: {err}", flush=True, file=sys.stderr)
    finally:
        if interactive:
            logger.debug("waiting for interactive vm to shutdown")
            await proc.wait()

        if proc.returncode is None:
            logger.debug("killing guest vm")
            kill = await asyncio.create_subprocess_exec(
                "sudo", "kill", "-KILL", str(proc.pid)
            )
            await kill.wait()


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
