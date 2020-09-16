#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Read the `run.py` docblock first.  Then, review the docs for
`new_nspawn_opts` and `PopenArgs`, and invoke `{run,popen}_booted_nspawn`.

This file uses `systemd-nspawn` to boot up `systemd` as the container's PID
1, and later uses `nsenter` to execute `opts.cmd` in that container.

The API tries to follow `subprocess.Popen` and `subprocess.run`, with a few
notable exceptions:

  - The way that booting is set up right now, we end up spawning
    and managing two separate processes:
      * The boot process: the container's `systemd`, running under `nspawn`.
      * The client process: later `nsenter`ed into the `systemd`'s namespaces.
    For this reason, both functions return a tuple, with the first element
    for the client process, and the second element for the boot process.
    The lifetime of the boot process **should** be longer (in both
    directions) than that of the client process.

  - The boot process's `stdout` (aka the boot console) and `stderr` both get
    redirected to `popen_args.boot_console`.  This process takes no input.
    In systemd 243+, the redirecting happens via some PTYs, so you will
    likely see carriage returns in those streams.  File a `systemd` issue to
    emit `stty -onlcr` against its PTYs if you need this fixed.

  - You can configure pipes to the client process via `subprocess.PIPE`.
    However, this is **not** possible with `boot_console`.

    The reason is simply that this not a need we currently have (outside of
    tests), and supporting it well would create considerably complexity.

    In a nutshell, from the start of the `systemd` process, until its exit,
    we have to keep `boot_console` drained to avoid deadlocks.  To make
    this draining happen correctly, one of a few things needs to happen:

      - `boot_console` is drained by the kernel (i.e. a file or a terminal)

      - Everything here is converted to `async` code with considerable
        attention  to detail and a considerable increase in conceptual
        complexity.

      - A separate thread or subprocess is provided to drain the read
        end of this pipe. This is what we do in `test_boot_proc_results`,
        but writing generic code for this is complex and potentially
        risky (fork / thread stuff), so it was not done yet.

        Moreover, providing built-in draining support of this sort would
        significantly increase API complexity, see P126395382 for a very raw
        preview.

SECURITY NOTE: Just as with `systemd-nspawn`'s `--console=pipe`, this can
pass FDs pointed at your terminal into the container, allowing the guest to
synthesize keystrokes on the host.
"""
import functools
import os
import signal
import subprocess
import textwrap
import time
from contextlib import contextmanager
from typing import ContextManager, Iterable, Tuple

from antlir.common import get_file_logger
from antlir.send_fds_and_run import popen_and_inject_fds_after_sudo

from .args import PopenArgs, _NspawnOpts
from .cmd import _NspawnSetup, maybe_popen_and_inject_fds
from .common import DEFAULT_PATH_ENV
from .plugin_hooks import _popen_plugin_driver
from .plugins import NspawnPlugin


log = get_file_logger(__file__)

# This is a temporary mountpoint for the host's `/proc` inside the
# container.  It is unmounted and removed before the user command starts.
# However, it may be visible to early boot-time units.
_OUTER_PROC = "/outerproc_boot"  # Distinct from `/outerproc_repo_server`


def run_booted_nspawn(
    opts: _NspawnOpts,
    popen_args: PopenArgs,
    *,
    plugins: Iterable[NspawnPlugin] = (),
) -> Tuple[subprocess.CompletedProcess, subprocess.CompletedProcess]:
    """
    The first `CompletedProcess` reflects for the user command `opts.cmd`
    that we tried to run in the booted container.

    The second one is for the `systemd` process representing the container
    boot process itself.
    """
    with popen_booted_nspawn(opts, popen_args, plugins=plugins) as (nsp, bp):
        ns_stdout, ns_stderr = nsp.communicate()
        # We don't make any provisions for pipes to the boot process,
        # see the file docblock.
    return (
        subprocess.CompletedProcess(
            args=nsp.args,
            returncode=nsp.returncode,
            stdout=ns_stdout,
            stderr=ns_stderr,
        ),
        subprocess.CompletedProcess(
            args=bp.args,
            returncode=bp.returncode,
            # These cannot be `subprocess.PIPE` per the file docblock.
            stdout=None,
            stderr=None,
        ),
    )


def popen_booted_nspawn(
    opts: _NspawnOpts,
    popen_args: PopenArgs,
    *,
    plugins: Iterable[NspawnPlugin] = (),
) -> Iterable[Tuple[subprocess.Popen, subprocess.Popen]]:
    return _popen_plugin_driver(
        opts=opts,
        popen_args=popen_args,
        post_setup_popen=_post_setup_popen_booted_nspawn,
        plugins=plugins,
    )


@contextmanager
def _post_setup_popen_booted_nspawn(
    setup: _NspawnSetup,
) -> Iterable[Tuple[subprocess.Popen, subprocess.Popen]]:
    with _popen_boot_systemd(setup) as (
        boot_proc,
        systemd_pid,
    ), _popen_nsenter_into_systemd(
        setup, boot_proc, systemd_pid=systemd_pid
    ) as nsenter_proc:
        yield nsenter_proc, boot_proc


def _wrap_systemd_exec():
    return [
        "/bin/bash",
        "-eu",
        "-o",
        "pipefail",
        "-c",
        # This script will be invoked with a writable FD forwarded into the
        # namespace this is being executed in as fd #3.
        #
        # It will then get the parent pid of the 'grep' process, which will be
        # the pid of the script itself (running as PID 1 inside the namespace),
        # so eventually the pid of systemd.
        #
        # We don't close the forwarded FD in this script. Instead we rely on
        # systemd to close all FDs it doesn't know about during its
        # initialization sequence.
        #
        # We rely on this because systemd will only close FDs after it creates
        # the /run/systemd/private socket (which makes systemctl usable) and
        # after setting up the necessary signal handlers to process the
        # SIGRTMIN+4 shutdown signal that we need to shut down the container
        # after invoking a command inside it.
        textwrap.dedent(
            f"""\
            grep ^PPid: {_OUTER_PROC}/self/status >&3
            umount -R {_OUTER_PROC}
            rmdir {_OUTER_PROC}
            exec /usr/lib/systemd/systemd --log-target=console
            """
        ),
    ]


@contextmanager
def _systemd_reaper(setup, boot_proc, systemd_pid):
    try:
        yield
    finally:
        log.info("User command exited, waiting to shut down systemd")
        # Signal until the `systemd` process exits, because the first signal
        # may arrive before it signal handler setup, and may be ignored.
        #
        # Future: we may want a timeout after which we send SIGKILL.
        delay = 0.005
        while boot_proc.poll() is None:
            # Terminate the container gracefully by sending a SIGRTMIN+4
            # directly to the `systemd` pid.
            #
            # We signal `systemd` instead of signaling its grandparent
            # `proc` since killing `sudo` or `systemd-nspawn` is unlikely to
            # result in a graceful shutdown, and might even leak the
            # `systemd` container.
            #
            # There's not too much for us to do about the race of "systemd
            # dies, some other process reuses its PID, we kill that too".
            # Let's just hope everyone uses 32-bit PIDs now.
            try:
                setup.subvol.run_as_root(
                    ["kill", "-s", str(signal.SIGRTMIN + 4), systemd_pid]
                )
                time.sleep(delay)
                delay = min(0.25, delay * 2)
            except subprocess.CalledProcessError:  # pragma: no cover
                pass  # Skip the wait if the PID is already invalid.


@contextmanager
def _popen_boot_systemd(
    setup: _NspawnSetup,
) -> Iterable[Tuple[subprocess.Popen, int]]:
    # We need `root` to boot `systemd`. The user for `opts.cmd` is set later.
    cmd = [*setup.nspawn_cmd, "--user=root"]
    # Instead of using the `--boot` argument to `systemd-nspawn` we are
    # going to ask systemd-nspawn to invoke a simple shell script so
    # that we can exfiltrate the process id of the process. After
    # sending that information out, the shell script `exec`s systemd.
    cmd.extend(
        [
            "--console=read-only",  # `stdin` is attached to `cmd` via `nsenter`
            f"--bind-ro=/proc:{_OUTER_PROC}",
            "--",
            *_wrap_systemd_exec(),
        ]
    )

    if setup.popen_args.boot_console == subprocess.PIPE:
        raise RuntimeError(
            "`popen_booted_nspawn` does not support `subprocess.PIPE` for "
            "the boot console. Please see the `booted.py` docblock for how to "
            "mitigate this."
        )

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've been
    # put into before we hand control over to the init system.
    exfil_r, exfil_w = os.pipe()
    with popen_and_inject_fds_after_sudo(
        cmd,
        [exfil_w],  # Forward the write fd of the pipe
        popen=functools.partial(
            # NB: If `boot_console` is None, this will redirect it to our
            # `stderr`, this is the right default for the most common use of
            # this API, which is to run a helper process in a container.
            # The result is that we get the helper's logs, but not `stdout`
            # contamination, so the parent remains usable in pipelines.
            setup.subvol.popen_as_root,
            check=setup.popen_args.check,
            env=setup.nspawn_env,
            stdin=subprocess.DEVNULL,  # We boot with `--console=read-only`
            stdout=setup.popen_args.boot_console,  # See `PopenArgs`
            # Only systemd logspam goes here. It would seem natural to
            # send this to `popen_args.stderr`, but this creates two issues:
            #   - (major) In this case, `stderr` would have to continue to
            #     exist even after the `nsenter`ed process exits, precluding
            #     us from using `subprocess.PIPE` or `communicate()` for
            #     `stderr` of the client process.  This is bad since it
            #     would increase user-visible complexity heftily -- the only
            #     way to consume stderr would be to do some dance with a
            #     separate consumer for the pipe that the file docblock
            #     recommends for `boot_console`.
            #   - (minor) The `stderr` of the client process may get
            #     polluted by nspawn.
            stderr=setup.popen_args.boot_console,
        ),
        set_listen_fds=True,
    ) as boot_proc:
        # Close the write fd of the pipe from this process so we
        # can read from this side.
        os.close(exfil_w)

        # We can't deadlock a piped `boot_console` -- `_wrap_systemd_exec`
        # does not write to stdout.  Writes to stderr should be minimal,
        # too, not enough to fill up a 64KiB default pipe buffer.
        with os.fdopen(exfil_r, "r") as f:
            systemd_pid = f.read().split(":")[1].strip()

        # From here onward, if either `stderr` and `boot_console` is a pipe,
        # then failing to drain the read end can deadlock.  See file docblock.

        log.info("Began systemd startup, injecting user command")
        with _systemd_reaper(setup, boot_proc, systemd_pid):
            yield boot_proc, systemd_pid


def _popen_nsenter_into_systemd(
    setup: _NspawnSetup, boot_proc: subprocess.Popen, *, systemd_pid: int
) -> ContextManager[subprocess.Popen]:
    opts = setup.opts

    # A set of default environment variables that should be
    # setup for the command inside the container.  This list models
    # what the default env looks for a shell launched inside
    # of systemd-nspawn.
    default_env = {
        "HOME": opts.user.pw_dir,
        "LOGNAME": opts.user.pw_name,
        "PATH": DEFAULT_PATH_ENV,
        "USER": opts.user.pw_name,
    }
    term_env = os.environ.get("TERM")
    if term_env is not None:
        default_env["TERM"] = term_env

    # Set the user properly for the nsenter'd command to run.
    # Future: consider properly logging in as the user with su
    # or something better so that a real user session is created
    # within the booted container.
    nsenter_cmd = [
        "/bin/bash",
        "-uec",
        # We have to enter into `systemd`'s cgroup, otherwise this `nsenter`'s
        # cgroup will be unmanageable via `systemd`' view of `/sys/fs/cgroup`,
        # and it will be unable to move it into a user session. The specific
        # failure mode is described by `SlowSudoTestCase`.
        #
        # Note that this runs on the host, so it's OK to assume that cgroup2
        # is mounted at the usual location.
        f"""
        echo $$ > "$(
            sed 's|^0::|/sys/fs/cgroup/|' < /proc/{systemd_pid}/cgroup
        )"/cgroup.procs
        exec "$@"
        """,
        "bash",  # $0 for `bash` above
        "nsenter",
        f"--target={systemd_pid}",
        "--all",
        f"--setuid={opts.user.pw_uid}",
        f"--setgid={opts.user.pw_gid}",
        # Clear and set the new env
        "env",
        "-",
        *setup.cmd_env,
        # At present, these defaults override any user-provided values.
        # This can be relaxed if there's a good reason.
        *(f"{k}={v}" for k, v in default_env.items()),
        *opts.cmd,
    ]

    # This never returns a bare Popen, so it's fine not to use @contextmanager
    return maybe_popen_and_inject_fds(
        nsenter_cmd,
        opts,
        popen=functools.partial(
            # NB: stdout is stderr if stdout is None, this is also our API.
            setup.subvol.popen_as_root,
            check=setup.popen_args.check,
            # We don't bind `env` because that's set via `nsenter_cmd`.
            stdin=setup.popen_args.stdin,
            stdout=setup.popen_args.stdout,
            stderr=setup.popen_args.stderr,
        ),
        # This command is not executing `systemd-nspawn`.
        set_listen_fds=False,
    )
