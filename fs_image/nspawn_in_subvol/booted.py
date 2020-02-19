#!/usr/bin/env python3
'''
No externally useful functions here.  Read the `run.py` docblock instead.

This file uses `systemd-nspawn` to boot up `systemd` as the container's PID
1, and later uses `nsenter` to execute `opts.cmd` in the container

Security note: Just as with `systemd-nspawn`'s `--console=pipe`, this can
pass FDs pointed at your terminal into the container, allowing the guest to
synthesize keystrokes on the host.
'''
import functools
import os
import signal
import subprocess
import sys

from send_fds_and_run import popen_and_inject_fds_after_sudo

from .cmd import _NspawnSetup
from .common import _wrap_systemd_exec


class BootedCompletedProcess(subprocess.CompletedProcess):
    def __init__(self, boot_proc, args, returncode, stdout, stderr):
        self.boot = boot_proc
        super().__init__(
            args=args,
            returncode=returncode,
            stdout=stdout,
            stderr=stderr
        )


def _run_booted_nspawn(
    setup: _NspawnSetup, popen
) -> subprocess.CompletedProcess:
    opts = setup.opts
    # We need `root` to boot `systemd`. The user for `opts.cmd` is set later.
    cmd = [*setup.nspawn_cmd, '--user=root']
    # Instead of using the `--boot` argument to `systemd-nspawn` we are
    # going to ask systemd-nspawn to invoke a simple shell script so
    # that we can exfiltrate the process id of the process. After
    # sending that information out, the shell script execs systemd.
    cmd.extend([
        '--bind-ro=/proc:/outerproc',
        '--',
        *_wrap_systemd_exec(),
    ])

    # Create a partial of the popen with stdout/stderr setup as
    # requested for the boot process.
    boot_popen = functools.partial(popen,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've been
    # put into before we hand control over to the init system.
    exfil_r, exfil_w = os.pipe()
    with (
        popen_and_inject_fds_after_sudo(
            cmd,
            [exfil_w],  # Forward the write fd of the pipe
            boot_popen,
            set_listen_fds=True
        )
    ) as boot_proc:
        # Close the write fd of the pipe from this process so we
        # can read from this side.
        os.close(exfil_w)

        with os.fdopen(exfil_r, 'r') as f:
            systemd_pid = f.read().split(':')[1].strip()

        # A set of default environment variables that should be
        # setup for the command inside the container.  This list models
        # what the default env looks for a shell launched inside
        # of systemd-nspawn.
        default_env = {
            'HOME': opts.user.pw_dir,
            'LOGNAME': opts.user.pw_name,
            'PATH': '/usr/local/bin:/usr/bin:/usr/local/sbin:/usr/sbin',
            'USER': opts.user.pw_name,
            'TERM': os.environ.get('TERM')
        }

        # Set the user properly for the nsenter'd command to run.
        # Future: consider properly logging in as the user with su
        # or something better so that a real user session is created
        # within the booted container.
        nsenter_cmd = [
            'nsenter',
            f'--target={systemd_pid}', '--all',
            f'--setuid={opts.user.pw_uid}',
            f'--setgid={opts.user.pw_gid}',
            # Clear and set the new env
            'env', '-',
            *setup.cmd_env,
            # At present, these defaults override any user-provided values.
            # This can be relaxed if there's a good reason.
            *(f'{k}={v}' for k, v in default_env.items()),
            *opts.cmd,
        ]

        with (
            popen_and_inject_fds_after_sudo(
                nsenter_cmd,
                opts.forward_fd,
                popen,
                set_listen_fds=True,
            ) if opts.forward_fd else popen(nsenter_cmd)
        ) as nsenter_proc:
            nsenter_stdout, nsenter_stderr = nsenter_proc.communicate()

        # Terminate the container gracefully by sending a
        # SIGRTMIN+4 directly to the systemd pid.
        setup.subvol.run_as_root(
            ['kill', '-s', str(signal.SIGRTMIN + 4), systemd_pid])

        boot_stdout, boot_stderr = boot_proc.communicate()

        # this is uncovered because this is only useful for manually
        # debugging
        if opts.debug_only_opts.boot_console_stdout:  # pragma: no cover
            sys.stdout.buffer.write(boot_stdout)

    # What we care about is the return status/data from the cmd we
    # wanted to execute.
    return BootedCompletedProcess(
        boot_proc=subprocess.CompletedProcess(
            args=boot_proc.args,
            returncode=boot_proc.returncode,
            stdout=boot_stdout,
            stderr=boot_stderr,
        ),
        args=nsenter_proc.args,
        returncode=nsenter_proc.returncode,
        stdout=nsenter_stdout,
        stderr=nsenter_stderr,
    )
