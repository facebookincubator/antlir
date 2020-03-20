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

from contextlib import contextmanager

from fs_image.common import get_file_logger, pipe
from fs_image.fs_utils import Path, temp_dir
from rpm.yum_dnf_from_snapshot import (
    launch_repo_servers_in_netns,
    prepare_isolated_yum_dnf_conf,
)
from rpm.yum_dnf_conf import YumDnf
from send_fds_and_run import popen_and_inject_fds_after_sudo


log = get_file_logger(__file__)
REPO_SERVER_CONFIG_DIR = Path('/repo-server')


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
    repo_server_config
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
            f'--bind-ro={repo_server_config}:{REPO_SERVER_CONFIG_DIR}',
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
def _write_yum_or_dnf_configs(
    yum_dnf, repo_server_config_dir, repo_server_snapshot_dir, host, port
):
    config_filename = {
        YumDnf.yum: Path('yum.conf'),
        YumDnf.dnf: Path('dnf.conf'),
    }[yum_dnf]
    plugin_directory = {
        YumDnf.yum: Path('yum/pluginconf.d'),
        YumDnf.dnf: Path('dnf/plugins'),
    }[yum_dnf]
    os.makedirs(repo_server_config_dir / plugin_directory)
    with open(
        repo_server_snapshot_dir / config_filename
    ) as in_conf, open(
        repo_server_config_dir / config_filename, 'w'
    ) as out_conf, prepare_isolated_yum_dnf_conf(
        yum_dnf,
        in_conf,
        out_conf,
        host,
        port,
        REPO_SERVER_CONFIG_DIR / plugin_directory,
        REPO_SERVER_CONFIG_DIR / config_filename,
    ):
        yield


@contextmanager
def _write_yum_and_dnf_configs(
    repo_server_config_dir, repo_server_snapshot_dir, host, port
):
    with _write_yum_or_dnf_configs(
        YumDnf.yum,
        repo_server_config_dir,
        repo_server_snapshot_dir,
        host,
        port,
    ), _write_yum_or_dnf_configs(
        YumDnf.dnf,
        repo_server_config_dir,
        repo_server_snapshot_dir,
        host,
        port,
    ):
        yield


@contextmanager
def _popen_and_inject_repo_server(
    nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
    repo_server_snapshot_dir, *, debug
):
    # We're running a repo-server with a socket inside the network
    # namespace.

    with temp_dir() as repo_server_config_dir:
        with _exfiltrate_container_pid_and_wait_for_ready(
            nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
            repo_server_config_dir
        ) as (
            container_pid, cmd_proc, send_ready
        ), launch_repo_servers_in_netns(
            target_pid=container_pid,
            repo_server_bin=repo_server_snapshot_dir / 'repo-server',
            snapshot_dir=repo_server_snapshot_dir,
            debug=debug,
        ) as hostports, _write_yum_and_dnf_configs(
            repo_server_config_dir,
            repo_server_snapshot_dir,
            # TEMPORARY: The generated config just uses the first
            # host:port.  Stacked diffs will rip out config generation
            # and use the config that was included in the snapshot.
            *hostports[0],
        ):
            send_ready()
            yield cmd_proc
