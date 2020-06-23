#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Serve RPM repo snapshots inside the container by adding this to `plugins`
kwarg of the `run_*` or `popen_*` functions:
  `nspawn_plugin_to_inject_repo_servers(snapshot_paths)`

The snapshots must already be in the container's image, and must have been
built by the `rpm_repo_snapshot()` target, and installed via
`install_rpm_repo_snapshot()`.
'''
import functools
import os
import textwrap

from contextlib import contextmanager, ExitStack
from dataclasses import dataclass
from io import BytesIO
from typing import Any, Iterable, List, Optional, Tuple

from fs_image.common import get_file_logger, pipe
from fs_image.fs_utils import Path

from .args import _NspawnOpts, PopenArgs
from .common import _OuterPopenCtxMgr, NspawnPlugin
from .launch_repo_servers import launch_repo_servers_for_netns


log = get_file_logger(__file__)

# This is a temporary mountpoint for the host's `/proc` inside the
# container.  It is unmounted and removed before the user command starts.
# However, in the booted case, it may be visible to early boot-time units.
_OUTER_PROC = '/outerproc_repo_server'  # Distinct from `/outerproc_boot`


@dataclass
class _ContainerPidExfiltrator:

    exfil_r: BytesIO
    exfil_w: BytesIO  # Must stay open until the container starts
    exfil_w_dest_fd: int  # Forward the prior file to this FD in the container
    ready_r: BytesIO  # Must stay open until the container starts
    ready_r_dest_fd: int  # Forward the prior file to this FD in the container
    ready_w: BytesIO

    # `None` means that `exfiltrate_container_pid` has not yet been called.
    _ready_sent: Optional[bool] = None

    @classmethod
    @contextmanager
    def new(
        cls, exfil_w_dest_fd: int, ready_r_dest_fd: int,
    ) -> '_ContainerPidExfiltrator':
        # The first pipe's write end is forwarded into the container, and
        # will be used to exfiltrated data about its PID namespace, before
        # we start the user command.
        #
        # The read end of the second pipe signals our exfiltration script
        # that it should continue to execute the user command.
        with pipe() as (exfil_r, exfil_w), pipe() as (ready_r, ready_w):
            yield _ContainerPidExfiltrator(
                exfil_r=exfil_r,
                exfil_w=exfil_w,
                exfil_w_dest_fd=exfil_w_dest_fd,
                ready_r=ready_r,
                ready_r_dest_fd=ready_r_dest_fd,
                ready_w=ready_w,
            )

    def wrap_user_cmd(self, cmd) -> List[str]:
        # This script will exfiltrate the outer PID of a process inside the
        # container's namespace. (See _wrap_systemd_exec for more details.)
        #
        # After sending this information, it will block on the "ready" FD to
        # wait for this script to complete setup.  Once the "ready" signal is
        # received, it will continue to execute the final command.
        wrap = textwrap.dedent(f'''\
            grep ^PPid: {_OUTER_PROC}/self/status >&{self.exfil_w_dest_fd}
            umount -R {_OUTER_PROC}
            rmdir {_OUTER_PROC}
            exec {self.exfil_w_dest_fd}>&-  # See note about `nsenter` below
            read line <&{self.ready_r_dest_fd}
            if [[ "$line" != "ready" ]] ; then
                echo 'Did not get ready signal' >&2
                exit 1
            fi
            exec {self.ready_r_dest_fd}<&-
            exec "$@"
        ''')
        return ['/bin/bash', '-eu', '-o', 'pipefail', '-c', wrap, '--', *cmd]

    @contextmanager
    def exfiltrate_container_pid(self) -> int:
        'Yields the outer PID of a process inside the container.'
        assert self._ready_sent is None, 'exfiltrate_container_pid called twice'
        self._ready_sent = False

        self.exfil_w.close()
        self.ready_r.close()

        try:
            # Note: this is `readline()` instead of `read()` because in the
            # booted case, we cannot wait for `exfil_w` to get closed, the
            # `nsenter` process also inherits it, and will hold it open for
            # as long as the user command runs, causing us to deadlock here.
            yield int(self.exfil_r.readline().decode().split(':')[1].strip())
        finally:
            if not self._ready_sent:
                self.send_ready()  # pragma: no cover

    def send_ready(self):
        assert self._ready_sent is False, \
            'Can only send ready once, after calling `exfiltrate_container_pid`'
        self.ready_w.write(b'ready\n')
        self.ready_w.close()
        self._ready_sent = True


@contextmanager
def _wrap_opts_with_container_pid_exfiltrator(opts: _NspawnOpts) -> Tuple[
    _NspawnOpts, _ContainerPidExfiltrator,
]:
    with _ContainerPidExfiltrator.new(
        # Below, we append FDs to `forward_fd`.  In the container, these
        # will map sequentially to `3 + len(opts.forward_fd)` and up.
        exfil_w_dest_fd=3 + len(opts.forward_fd),
        ready_r_dest_fd=4 + len(opts.forward_fd),
    ) as cpe:
        yield opts._replace(
            forward_fd=(
                *opts.forward_fd,
                # The order of the appended FDs must match `*_dest_fd` above.
                cpe.exfil_w.fileno(),
                cpe.ready_r.fileno(),
            ),
            bindmount_ro=(*opts.bindmount_ro, ('/proc', _OUTER_PROC)),
            cmd=cpe.wrap_user_cmd(opts.cmd),
        ), cpe


def _inject_repo_servers(
    serve_rpm_snapshots: Iterable[Path], popen: _OuterPopenCtxMgr,
) -> _OuterPopenCtxMgr:
    'Wraps `_outer_popen_{,non_}booted_nspawn`.'

    @functools.wraps(popen)
    @contextmanager
    def wrapped_popen(opts: _NspawnOpts, popen_args: PopenArgs):
        with ExitStack() as stack:
            # Future: bring this back, so we don't have to install it into
            # the snapshot.  The reason this is commented out for now is
            # that the FB-internal repo-server is a bit expensive to build,
            # and so we prefer to release and package it with the BA to hide
            # the cost.
            #
            # However, once `image.released_layer` is a thing, it would be
            # pretty easy to release just the expensive-to-build part inside
            # FB, and have the rest be built "live".
            #
            # On balance, a "live-built" `repo-server` is easiest to work
            # with, since you can edit the code, and try it in @mode/dev
            # without rebuilding anything.  The only downside is that
            # changes to the `repo-server` <-> snapshot interface require a
            # simultaneous commit of both, but we do this very rarely.
            #
            # For now, the snapshot must contain the repo-server (below).
            #
            # repo_server_bin = stack.enter_context(Path.resource(
            #    __package__, 'repo-server', exe=True,
            # ))
            # Rewrite `opts` with a plugin script and some forwarded FDs
            opts, cpe = stack.enter_context(
                _wrap_opts_with_container_pid_exfiltrator(opts)
            )
            popen_res = stack.enter_context(popen(opts, popen_args))
            container_pid = stack.enter_context(cpe.exfiltrate_container_pid())
            for snap_dir in serve_rpm_snapshots:
                # NB: When `opts.snapshot` is set, `opts.layer` is not the
                # container's actual subvol, but its read-only predecessor.
                # This effectively means that in the "normal" case of
                # `opts.layer` being a build artifact, the container cannot
                # affect the contents of the snapshot.  This seems okay.
                stack.enter_context(launch_repo_servers_for_netns(
                    target_pid=container_pid,
                    repo_server_bin=opts.layer.path(snap_dir / 'repo-server'),
                    snapshot_dir=opts.layer.path(snap_dir),
                    debug=opts.debug_only_opts.debug,
                ))
            cpe.send_ready()
            yield popen_res

    return wrapped_popen


def nspawn_plugin_to_inject_repo_servers(
    serve_rpm_snapshots: Iterable[Path],
) -> NspawnPlugin:
    serve_rpm_snapshots = tuple(serve_rpm_snapshots)
    return NspawnPlugin(
        popen=functools.partial(_inject_repo_servers, serve_rpm_snapshots)
            if serve_rpm_snapshots else None,
    )
