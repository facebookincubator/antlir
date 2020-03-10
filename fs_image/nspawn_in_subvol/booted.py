#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Read the `run.py` docblock first.  Then, review the docs for
`new_nspawn_opts` and `PopenArgs`, and invoke `run_booted_nspawn`.

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
import textwrap

from send_fds_and_run import popen_and_inject_fds_after_sudo

from .args import _NspawnOpts, PopenArgs
from .cmd import maybe_popen_and_inject_fds, _NspawnSetup, _nspawn_setup


class BootedCompletedProcess(subprocess.CompletedProcess):
    def __init__(self, boot_proc, args, returncode, stdout, stderr):
        self.boot = boot_proc
        super().__init__(
            args=args,
            returncode=returncode,
            stdout=stdout,
            stderr=stderr
        )


def run_booted_nspawn(
    opts: _NspawnOpts, popen_args: PopenArgs
) -> BootedCompletedProcess:
    with _nspawn_setup(opts, popen_args) as setup:
        return _run_booted_nspawn(setup)


def _wrap_systemd_exec():
    return [
        '/bin/bash', '-eu', '-o', 'pipefail', '-c',
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
        textwrap.dedent('''\
            grep ^PPid: /outerproc/self/status >&3
            umount -R /outerproc
            rmdir /outerproc
            exec /usr/lib/systemd/systemd
        '''),
    ]


def _run_booted_nspawn(setup: _NspawnSetup) -> BootedCompletedProcess:
    opts = setup.opts
    # We need `root` to boot `systemd`. The user for `opts.cmd` is set later.
    cmd = [*setup.nspawn_cmd, '--user=root']
    # Instead of using the `--boot` argument to `systemd-nspawn` we are
    # going to ask systemd-nspawn to invoke a simple shell script so
    # that we can exfiltrate the process id of the process. After
    # sending that information out, the shell script execs systemd.
    cmd.extend([
        '--console=read-only',  # `stdin` is attached to `cmd` via `nsenter`
        '--bind-ro=/proc:/outerproc',
        '--',
        *_wrap_systemd_exec(),
    ])

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've been
    # put into before we hand control over to the init system.
    exfil_r, exfil_w = os.pipe()
    with popen_and_inject_fds_after_sudo(
        cmd,
        [exfil_w],  # Forward the write fd of the pipe
        popen=functools.partial(
            # NB: If `boot_console` is None, this will redirect it to
            # `stderr`, this is the right default for the most common use of
            # this API, which is to run a helper process in a container.
            # The result is that we get the helper's logs, but not `stdout`
            # contamination, so the parent remains usable in pipelines.
            setup.subvol.popen_as_root,
            check=setup.popen_args.check,
            env=setup.nspawn_env,
            stdin=subprocess.DEVNULL,  # We boot with `--console=read-only`
            stdout=setup.popen_args.boot_console,  # See `PopenArgs`
            stderr=setup.popen_args.stderr,  # Only systemd logspam goes here
        ),
        set_listen_fds=True
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

        with maybe_popen_and_inject_fds(
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
        ) as nsenter_proc:
            nsenter_stdout, nsenter_stderr = nsenter_proc.communicate()

        # Terminate the container gracefully by sending a
        # SIGRTMIN+4 directly to the systemd pid.
        setup.subvol.run_as_root(
            ['kill', '-s', str(signal.SIGRTMIN + 4), systemd_pid])

        boot_stdout, boot_stderr = boot_proc.communicate()

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
