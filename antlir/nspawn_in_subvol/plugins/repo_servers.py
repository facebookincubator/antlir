#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Serve RPM repo snapshots inside the container by adding this to `plugins`
kwarg of the `run_*` or `popen_*` functions: `RepoServers(snapshot_paths)`

In practice, you will want `rpm_nspawn_plugins` instead.

The snapshots must already be in the container's image, and must have been
built by the `rpm_repo_snapshot()` target, and installed via
`install_rpm_repo_snapshot()`.
"""
import textwrap
from contextlib import ExitStack, contextmanager
from dataclasses import dataclass
from io import BytesIO
from typing import Iterable, List, Optional, Tuple

from antlir.common import get_logger, pipe
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import PopenArgs, _NspawnOpts
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
    _PopenResult,
    _PostSetupPopenCtxMgr,
)
from antlir.subvol_utils import Subvol

from . import NspawnPlugin
from .launch_repo_servers import launch_repo_servers_for_netns


log = get_logger()

# This is a temporary mountpoint for the host's `/proc` inside the
# container.  It is unmounted and removed before the user command starts.
# However, in the booted case, it may be visible to early boot-time units.
_OUTER_PROC = "/outerproc_repo_server"


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
        cls, exfil_w_dest_fd: int, ready_r_dest_fd: int
    ) -> "_ContainerPidExfiltrator":
        # The first pipe's write end is forwarded into the container, and
        # will be used to exfiltrated data about its PID namespace, before
        # we start the user command.
        #
        # The read end of the second pipe signals our exfiltration script
        # that it should continue to execute the user command.
        with pipe() as (exfil_r, exfil_w), pipe() as (ready_r, ready_w):
            # pyre-fixme[7]: Expected `_ContainerPidExfiltrator` but got
            #  `Generator[_ContainerPidExfiltrator, None, None]`.
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
        wrap = textwrap.dedent(
            f"""\
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
        """
        )
        return ["/bin/bash", "-eu", "-o", "pipefail", "-c", wrap, "--", *cmd]

    @contextmanager
    def exfiltrate_container_pid(self) -> int:
        "Yields the outer PID of a process inside the container."
        assert self._ready_sent is None, "exfiltrate_container_pid called twice"
        self._ready_sent = False

        self.exfil_w.close()
        self.ready_r.close()

        try:
            # Note: this is `readline()` instead of `read()` because we cannot
            # wait for `exfil_w` to get closed, the `nsenter` process also
            # inherits it, and will hold it open for as long as the user command
            # runs, causing us to deadlock here.
            #
            # pyre-fixme[7]: Expected `int` but got `Generator[int, None,
            # None]`.
            yield int(self.exfil_r.readline().decode().split(":")[1].strip())
        finally:
            if not self._ready_sent:
                self.send_ready()  # pragma: no cover

    def send_ready(self) -> None:
        assert (
            self._ready_sent is False
        ), "Can only send ready once, after calling `exfiltrate_container_pid`"
        self.ready_w.write(b"ready\n")
        self.ready_w.close()
        self._ready_sent = True


@contextmanager
def _wrap_opts_with_container_pid_exfiltrator(
    opts: _NspawnOpts,
) -> Tuple[_NspawnOpts, _ContainerPidExfiltrator]:
    # pyre-fixme[16]: `_ContainerPidExfiltrator` has no attribute `__enter__`.
    with _ContainerPidExfiltrator.new(
        # Below, we append FDs to `forward_fd`.  In the container, these
        # will map sequentially to `3 + len(opts.forward_fd)` and up.
        # pyre-fixme[6]: Expected `Sized` for 1st param but got `Iterable[int]`.
        exfil_w_dest_fd=3 + len(opts.forward_fd),
        # pyre-fixme[6]: Expected `Sized` for 1st param but got `Iterable[int]`.
        ready_r_dest_fd=4 + len(opts.forward_fd),
    ) as cpe:
        # pyre-fixme[7]: Expected `Tuple[_NspawnOpts, _ContainerPidExfiltrator]`
        #  but got `Generator[Tuple[_NspawnOpts, typing.Any], None, None]`.
        yield opts._replace(
            forward_fd=(
                *opts.forward_fd,
                # The order of the appended FDs must match `*_dest_fd` above.
                cpe.exfil_w.fileno(),
                cpe.ready_r.fileno(),
            ),
            bindmount_ro=(*opts.bindmount_ro, ("/proc", _OUTER_PROC)),
            cmd=cpe.wrap_user_cmd(opts.cmd),
        ), cpe


class RepoServers(NspawnPlugin):
    def __init__(self, serve_rpm_snapshots: Iterable[Path]) -> None:
        self._serve_rpm_snapshots = serve_rpm_snapshots

    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        subvol: Subvol,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> _NspawnSetup:
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
        # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
        with _wrap_opts_with_container_pid_exfiltrator(opts) as (
            opts,
            cpe,
            # pyre-fixme[19]: Expected 2 positional arguments.
        ), setup_ctx(subvol, opts, popen_args) as setup:
            # pyre-fixme[16]: `RepoServers` has no attribute
            #  `_container_pid_exfiltrator`.
            self._container_pid_exfiltrator = cpe
            # pyre-fixme[7]: Expected `_NspawnSetup` but got
            #  `Generator[antlir.nspawn_in_subvol.cmd._NspawnSetup, None,
            #  None]`.
            yield setup

    @contextmanager
    def wrap_post_setup_popen(
        self, post_setup_popen_ctx: _PostSetupPopenCtxMgr, setup: _NspawnSetup
    ) -> _PopenResult:
        snap_subvol = setup.subvol

        with ExitStack() as stack:
            popen_res = stack.enter_context(post_setup_popen_ctx(setup))
            container_pid = stack.enter_context(
                # pyre-fixme[16]: `RepoServers` has no attribute
                #  `_container_pid_exfiltrator`.
                self._container_pid_exfiltrator.exfiltrate_container_pid()
            )

            # Canonicalize paths here and below to ensure that it doesn't
            # matter if snapshots are specified by symlink or by real location.
            # This must occur after `AttachAntlirDir.wrap_setup_subvol`
            # so that we can resolve symlinks in `__antlir__`.
            serve_rpm_snapshots = frozenset(
                snap_subvol.canonicalize_path(p)
                for p in self._serve_rpm_snapshots
            )

            # To speed up startup, launch all the servers, and then await them.
            snap_to_servers = {
                snap_dir: stack.enter_context(
                    # pyre-fixme[6]: Expected `ContextManager[Variable[
                    # contextlib._T]...
                    launch_repo_servers_for_netns(
                        target_pid=container_pid,
                        snapshot_dir=snap_subvol.path(snap_dir),
                        repo_server_bin=snap_subvol.path(
                            snap_dir / "repo-server"
                        ),
                    )
                )
                for snap_dir in serve_rpm_snapshots
            }
            log.info(
                "Started `repo-server` for snapshots (ports): "
                + ", ".join(
                    f"""{snap.basename()} ({' '.join(
                        str(s.port) for s in servers
                    )})"""
                    for snap, servers in snap_to_servers.items()
                )
            )
            self._container_pid_exfiltrator.send_ready()
            # pyre-fixme[7]: Expected `Tuple[subprocess.Popen[typing.Any],
            #  subprocess.Popen[typing.Any]]` but got
            #  `Generator[Tuple[subprocess.Popen[typing.Any],
            #  subprocess.Popen[typing.Any]], None, None]`.
            yield popen_res
