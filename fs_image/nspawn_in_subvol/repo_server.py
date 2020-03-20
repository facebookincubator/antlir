#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
No externally useful functions here.  Read the `run.py` docblock instead.

This file sets up the container with an RPM repo server to serve snapshots
made by `fs_image/rpm/snapshot_repos.py` inside the container.

Fixme: this is currently tightly coupled to non_booted.py, but that'll
change on a later diff.
'''
import os
import textwrap

from contextlib import contextmanager, ExitStack
from typing import Iterable

from fs_image.common import get_file_logger, pipe
from fs_image.fs_utils import Path
from rpm.yum_dnf_from_snapshot import launch_repo_servers_in_netns
from send_fds_and_run import popen_and_inject_fds_after_sudo


log = get_file_logger(__file__)


def _exfiltrate_container_pid_and_wait_for_ready_cmd(exfil_fd, ready_fd):
    return [
        '/bin/bash', '-eu', '-o', 'pipefail', '-c',
        # This script will exfiltrate the outer PID of a process inside the
        # container's namespace. (See _wrap_systemd_exec for more details.)
        #
        # After sending this information, it will block on the "ready" FD to
        # wait for this script to complete setup.  Once the "ready" signal is
        # received, it will continue to execute the final command.
        textwrap.dedent(f'''\
            grep ^PPid: /outerproc/self/status >&{exfil_fd}
            umount -R /outerproc
            rmdir /outerproc
            exec {exfil_fd}>&-
            read line <&{ready_fd}
            if [[ "$line" != "ready" ]] ; then
                echo 'Did not get ready signal' 1>&2
                exit 1
            fi
            exec {ready_fd}<&-
            exec "$@"
        '''),
        # pass 'bash' as $0, then opts.cmd will become the additional args in
        # $@ for the final `exec` command in the script above.
        'bash',
    ]


@contextmanager
def _exfiltrate_container_pid_and_wait_for_ready(
    nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
):
    cmd = list(nspawn_cmd)

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've
    # been put into before we hand control over to the init system.
    #
    # And a pipe that we can use to signal the bash script that it
    # should go ahead and exec the final command.
    with pipe() as (exfil_r, exfil_w), pipe() as (ready_r, ready_w):

        # We'll add the read end of the pipe to the end of forward_fds,
        # which will then start at FD (3 + len(opts.forward_fd)) inside
        # the subprocess.
        forward_fds = list(forward_fds)
        exfil_fd = 3 + len(forward_fds)
        ready_fd = 4 + len(forward_fds)
        forward_fds.extend([exfil_w.fileno(), ready_r.fileno()])

        cmd.extend([
            '--bind-ro=/proc:/outerproc',
            '--',
        ])
        cmd.extend(
            _exfiltrate_container_pid_and_wait_for_ready_cmd(
                exfil_fd, ready_fd)
        )
        cmd.extend(container_cmd)
        with popen_and_inject_fds_after_sudo(
            cmd, forward_fds, popen_for_nspawn, set_listen_fds=True
        ) as cmd_proc:
            exfil_w.close()
            ready_r.close()

            # outer PID of a process inside the container.
            container_pid = int(exfil_r.read().decode().split(':')[1].strip())
            exfil_r.close()

            ready_sent = False

            def send_ready():
                nonlocal ready_sent
                if ready_sent:
                    raise RuntimeError(  # pragma: no cover
                        "Can't send ready twice"
                    )
                ready_w.write(b'ready\n')
                ready_w.close()
                ready_sent = True

            try:
                yield container_pid, cmd_proc, send_ready
            finally:
                if not ready_sent:
                    send_ready()  # pragma: no cover


@contextmanager
def _popen_and_inject_repo_servers(
    nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
    serve_rpm_snapshots: Iterable[Path], *, debug
):
    with ExitStack() as stack:
        # We're running repo-servers with a socket inside the network namespace.
        container_pid, cmd_proc, send_ready = stack.enter_context(
            _exfiltrate_container_pid_and_wait_for_ready(
                nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
            )
        )
        for serve_rpm_snapshot in serve_rpm_snapshots:
            stack.enter_context(launch_repo_servers_in_netns(
                target_pid=container_pid,
                repo_server_bin=serve_rpm_snapshot / 'repo-server',
                snapshot_dir=serve_rpm_snapshot,
                debug=debug,
            ))
        send_ready()
        yield cmd_proc
