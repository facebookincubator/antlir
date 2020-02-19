#!/usr/bin/env python3
'''
No externally useful functions here.  Read the `run.py` docblock instead.

This file uses `systemd-nspawn --as-pid2` to run nspawn's internal "stub
init" as PID 1 of the container, and have that start `opts.cmd` as PID 2.

Security note: We use `--console=pipe`, which means that FDs that point at
your terminal may make it inside the container, allowing the guest to
synthesize keystrokes on the host.
'''
import subprocess

from send_fds_and_run import popen_and_inject_fds_after_sudo

from .cmd import _NspawnSetup
from .common import _nspawn_version
from .repo_server import _popen_and_inject_repo_server


def _run_non_booted_nspawn(
    setup: _NspawnSetup, popen,
) -> subprocess.CompletedProcess:
    opts = setup.opts
    # Lets get the version locally right up front.  If this fails we'd like to
    # know early rather than later.
    version = _nspawn_version()

    cmd = [
        *setup.nspawn_cmd,
        # Add `--as-pid2` to run an nspawn-provided stub "init" process as
        # PID 1 of the container, which starts our actual workload as PID 2.
        # The command in `opts.cmd` is not (currently) meant to be an "init"
        # process.  And a PID 1 of a PID namespace must be a functioning
        # "init" at least insofar as signal handling is concerned.
        '--as-pid2',
        f'--user={opts.user.pw_name}',
    ]
    # This is last to let the user have final say over the environment.
    cmd.extend(['--setenv=' + se for se in setup.cmd_env])
    if version >= 242:
        # This essentially reverts to pre-242 behavior, where the container
        # has direct access to the caller's FDs 0/1/2, which may be the
        # running host's TTY/PTY (or other privileged FDs).  This is a
        # SECURITY RISK in the sense that if the container is running
        # untrusted code, it can now synthesize keystrokes on a host
        # terminal and escape.
        #
        # We leave it like this for a few reasons:
        #   - We currently only build trusted code with the toolchain.
        #     Securing it against untrusted code is a big effort, covering
        #     more than just this specific vulnerability.
        #   - Being able to use container invocations in pipelines is
        #     very useful.
        #   - In the boot case, we `nsenter`, which is subject to the
        #     same attack. We don't have code to interpose a PTY there.
        #   - If we wanted to mitigate the risk, we could later do so:
        #       * Add an `--interactive` mode that interposes a PTY, for
        #         when the user wants that.  Default to that when no command
        #         is given to the CLI, otherwise use `--non-interactive`.
        #       * In non-interactive mode, replace FDs 0/1/2 with
        #         something that interposes a pipe -- i.e. instead of
        #         `/dev/pts/0`, the container sees `[pipe]`, which
        #        is `splice`d to write to the original PTY.
        #     In fact, the mitigation really belongs in `systemd-nspawn`, we
        #     may yet propose it to upstream.
        cmd.append('--console=pipe')

    with (
        _popen_and_inject_repo_server(
            cmd,
            opts.cmd,
            opts.forward_fd,
            popen,
            opts.serve_rpm_snapshot_dir,
            debug=opts.debug_only_opts.debug,
        ) if opts.serve_rpm_snapshot_dir
        else (
            popen_and_inject_fds_after_sudo(
                cmd + ['--'] + opts.cmd,
                opts.forward_fd,
                popen,
                set_listen_fds=True,
            ) if opts.forward_fd
            else popen(cmd + ['--'] + opts.cmd)
        )
    ) as cmd_proc:
        cmd_stdout, cmd_stderr = cmd_proc.communicate()

    return subprocess.CompletedProcess(
        args=cmd_proc.args,
        returncode=cmd_proc.returncode,
        stdout=cmd_stdout,
        stderr=cmd_stderr,
    )
