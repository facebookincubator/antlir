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
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from contextlib import AsyncExitStack, asynccontextmanager, contextmanager
from enum import Enum
from itertools import chain
from typing import (
    AsyncContextManager,
    Iterable,
    NamedTuple,
    Optional,
    List,
    Union,
)

from antlir.common import init_logging, get_logger
from antlir.compiler.items.mount import mounts_from_image_meta
from antlir.config import load_repo_config
from antlir.fs_utils import Path
from antlir.shape import Shape
from antlir.tests.layer_resource import layer_resource_subvol
from antlir.unshare import Namespace, Unshare
from antlir.vm.guest_ssh import GuestSSHConnection
from antlir.vm.share import BtrfsDisk, Plan9Export, Share
from antlir.vm.tap import VmTap
from antlir.vm.vm_opts_t import vm_opts_t


logger = get_logger()


class VMBootError(Exception):
    """The VM failed to boot"""

    pass


def _wait_for_boot(sockfile: Path, timeout_ms: int = 300 * 1000) -> int:
    """
    The guest sends a READY message to the unix domain socket when it has
    reached the booted state.  This will wait for up to `timeout_sec` seconds
    the notify socket to actually show up and be connectable, and then wait
    for the remaining amount of `timeout_sec` for the actual boot event.

    This method returns the number of milliseconds that it took to receive the
    boot event from inside the VM.
    """
    start_ms = int(time.monotonic() * 1000)
    # The emulator is the thing that actually ends up creating the uds file, so
    # we might have to wait a bit for the file to show up.
    logger.debug(f"Waiting {timeout_ms}ms for notify socket: {sockfile}")
    try:
        elapsed_ms = sockfile.wait_for(timeout_ms=timeout_ms)
    except FileNotFoundError as fnfe:
        logger.debug(f"Notify socket never showed up: {fnfe}")
        raise VMBootError(f"Timeout waiting for notify socket: {fnfe}")

    # Calculate how much time we have left for the recv portion of this.
    # The socket APIs operate in seconds, so we have to convert, floats are ok.
    recv_timeout_sec = (timeout_ms - elapsed_ms) / 1000
    logger.debug(f"Connecting to notify socket: {sockfile}")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        try:
            logger.debug(
                f"Waiting {recv_timeout_sec:.4f}s for boot event: {sockfile}"
            )
            sock.settimeout(recv_timeout_sec)
            sock.connect(sockfile)

            # Note: we don't really read anything from this socket, we just wait
            # for *some* data to show up. In the future we could have the notify
            # service that runs in the VM actually send some useful data that
            # we could use.
            sock.recv(16)
            logger.debug("Guest boot event received")

            # Return "real" elapsed_ms since we started
            return int(time.monotonic() * 1000) - start_ms

        except socket.timeout:
            elapsed_ms = int(time.monotonic() * 1000) - start_ms
            logger.debug(f"socket timeout after {elapsed_ms}ms")
            raise VMBootError(f"Timeout waiting for boot event: {timeout_ms}ms")
        finally:
            logger.debug(f"Closing notify socket: {sockfile}")
            sock.close()


class ShellMode(Enum):
    console = "console"
    ssh = "ssh"

    def __str__(self):
        return self.value


ConsoleRedirect = Union[int, Path, None]
DEFAULT_CONSOLE = subprocess.DEVNULL
DEFAULT_TIMEOUT_MS = 300 * 1000


class VMExecOpts(Shape):
    """
    This is the common set of arguments that can be passed to an `antlir.vm`
    cli.
    """

    # Bind the repository root into the VM
    bind_repo_ro: bool = True
    # Where should the console output for the VM go?
    console: ConsoleRedirect = DEFAULT_CONSOLE
    # Extra, undefined arguments that are passed on the cli
    extra: List[str] = []
    # VM Opts instance passed to the CLI
    opts: vm_opts_t
    # Enable debug logs
    debug: bool = False
    # Connect to a shell inside the vm via the specified method
    shell: Optional[ShellMode] = None
    # How many millis to allow the VM to run.  The timeout starts
    # as soon as the emulator process is spawned.
    timeout_ms: int = DEFAULT_TIMEOUT_MS

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
            "--console",
            # This is used when the bare option with no arg is used.
            const=None,
            # This is used when no option is provided
            default=subprocess.DEVNULL,
            nargs="?",
            # This is used only when an argument is provided
            type=Path.from_argparse,
            help="Where to send console output. If --console=/path/to/file is "
            "provided, append the console output to the file.  If just "
            "--console is provided, send to stdout for easier debugging. "
            "By default the console output is supressed.",
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
            default=False,
            help="Show debug logs",
        )

        parser.add_argument(
            "--shell",
            choices=list(ShellMode),
            type=ShellMode,
            default=None,
            help="Connect to an interactive shell inside the VM via the "
            "specified method.  When this option is used, no additional "
            "commands or automation is allowed.  When you exit the shell "
            "the VM will terminate.",
        )

        parser.add_argument(
            "--timeout",
            dest="timeout_ms",
            # We want to allow the cli to specify seconds, mostly because that
            # is what external tools that invoke this will use.  But internally
            # we want to use millis, so we'll convert it right here to avoid
            # any confusion later.
            type=lambda t: int(t) * 1000,
            # Inside FB some tools set an env var instead of passing an
            # option. Maybe we can get rid of this if we fix the few
            # tools that call this.
            default=os.environ.get("TIMEOUT", DEFAULT_TIMEOUT_MS),
            help="How many seconds to allow the VM to run.  The clock starts "
            "as soon as the emulator is spawned.",
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
    timeout_ms: int = DEFAULT_TIMEOUT_MS,
    console: ConsoleRedirect = DEFAULT_CONSOLE,
    bind_repo_ro: bool = True,
    shell: Optional[ShellMode] = None,
    shares: Optional[Iterable[Share]] = None,
):
    notify_sockfile = Path(
        os.path.join(
            tempfile.gettempdir(), "vmtest_notify_" + uuid.uuid4().hex + ".sock"
        )
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
    mounts = mounts_from_image_meta(opts.disk.package.path)

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
            )  # pragma: no cover

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
                path=str(opts.disk.package.path),
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

    logger.debug(
        "Namespace has been created. Enter with: "
        f"{' '.join(ns.nsenter_as_user('/bin/bash'))}"
    )

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
        # socket/serial device pair (for use by _wait_for_boot)
        "-chardev",
        f"socket,path={notify_sockfile},id=notify,server",
        "-device",
        "virtserialport,chardev=notify,name=notify-host",
    ] + list(tapdev.qemu_args)

    # The bios to boot the emulator with
    args.extend(["-bios", str(opts.runtime.emulator.bios.path)])

    # Set the path for loading additional roms
    args.extend(["-L", str(opts.runtime.emulator.roms_dir.path)])

    if os.access("/dev/kvm", os.R_OK | os.W_OK):
        args += ["-enable-kvm"]
    else:  # pragma: no cover
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

    qemu_cmd = ns.nsenter_as_user(str(opts.runtime.emulator.binary.path), *args)

    # Special console handling here.
    # Future: This should really be done by the caller and provided as a
    # BytesIO type.
    if console and isinstance(console, Path):
        logger.debug(f"console is a file: {console}")
        console = stack.enter_context(console.open(mode="a"))
    try:
        logger.info(f"Booting VM using ShellMode: {shell}")
        # If we are asking for a shell, and more specifically a *console* shell
        # then we need to start the emulator process with stdin, stdout, and
        # stderr attached to the users tty.  This is a special case
        # Note: this is not covered by the test cases yet because the API does
        # not yet provide a good way to expose this.
        if shell and shell == ShellMode.console:  # pragma: no cover
            proc = subprocess.Popen(qemu_cmd)
        else:
            proc = subprocess.Popen(
                qemu_cmd,
                # Never have stdin connected unless we are in shell mode
                stdin=subprocess.DEVNULL,
                stdout=console,
                # Send stderr to the same place that console goes
                stderr=subprocess.STDOUT,
            )

        boot_elapsed_ms = _wait_for_boot(notify_sockfile, timeout_ms=timeout_ms)
        timeout_ms = timeout_ms - boot_elapsed_ms

        logger.debug(
            f"VM boot time: {boot_elapsed_ms}ms, "
            f"timeout_ms is now: {timeout_ms}ms"
        )
        logger.debug(f"VM ipv6: {tapdev.guest_ipv6_ll}")

        # Note: This logic is quite weird and convoluted.  It will get cleaner
        # in coming diffs.
        # Also Note: this is not covered in test cases because the API does
        # not yet provide a good way to expose this.
        if shell:  # pragma: no cover
            if shell == ShellMode.ssh:
                logger.debug("Using ShellMode == ShellMode.ssh")
                with GuestSSHConnection(
                    tapdev=tapdev, options=opts.runtime.connection.options
                ) as ssh:
                    ssh_cmd = ssh.ssh_cmd(timeout_ms=timeout_ms)
                    logger.debug(f"cmd: {' '.join(ssh_cmd)}")
                    shell_proc = subprocess.Popen(ssh_cmd)
                    shell_proc.wait()

                logger.debug(f"Shell has terminated: {shell_proc.returncode}")

            # Future: This is because the control flow is backwards.
            # Once this is inverted the specific behavior provided by
            # the current callers will be provided via a co-routine passed
            # to this method.  The co-routine will only be called if
            # necessary.
            yield (None, boot_elapsed_ms, timeout_ms)
        else:
            if opts.runtime.connection.scheme == "ssh":
                with GuestSSHConnection(
                    tapdev=tapdev, options=opts.runtime.connection.options
                ) as ssh:
                    yield (ssh, boot_elapsed_ms, timeout_ms)
            else:
                raise AttributeError(
                    "Invalid VM connect scheme: "
                    f"{opts.runtime.connection.scheme}"
                )  # pragma: no cover
    # Note: The error cases are not yet covered properly in tests.
    except VMBootError as vbe:  # pragma: no cover
        logger.error(f"VM failed to boot: {vbe}")
        raise RuntimeError(f"VM failed to boot: {vbe}")
    except Exception as e:  # pragma: no cover
        logger.error(f"VM failed: {e}")
        raise RuntimeError(f"VM failed: {e}")
    finally:

        # Future: unless we are running in `--shell=console` mode, the VM
        # hasn't been told to shutdown yet.  So this is the 'default'
        # behavior for termination, but really this should be a last resort
        # kind of thing.  The VM should terminate gracefully from within the
        # Guest OS if possible, and only if it doesn't terminate by itself
        # within the timeout, then we kill it like this.
        # Note: we can't easily test the console mode yet, so we don't cover
        # this branch
        if shell == ShellMode.console:  # pragma: nocover
            logger.info("Wait for VM to terminate via console")
            proc.wait()
        elif proc.returncode is None or shell == ShellMode.ssh:
            logger.debug(f"Sending kill to VM: {proc.pid}")

            kill = subprocess.run(
                ["sudo", "kill", "-KILL", str(proc.pid)],
                # Don't throw if this fails, the pid could have exited by the
                # time this runs
                check=False,
                capture_output=True,
                text=True,
            )
            logger.debug(f"VM -KILL returned with: {kill.returncode}")

            # Now we just wait
            logger.info("Wait for VM to terminate via kill")
            proc.wait()
        # This branch should never be reached, but we want to know if the elif
        # condition is sometimes not met.  But I don't know how to trigger it,
        # so we can't easily cover this.
        else:  # pragma: nocover
            raise RuntimeError(
                "Unknown VM termination state: "
                f"ret: {proc.returncode}, pid: {proc.pid}"
            )

        logger.debug(f"VM exited with: {proc.returncode}")


@asynccontextmanager
async def vm(*args, **kwargs) -> AsyncContextManager[GuestSSHConnection]:
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
