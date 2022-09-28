#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Serve RPM repo snapshots inside the container by adding this to `plugins`
kwarg of the `run_*` or `popen_*` functions: `RepoServers(snapshot_paths)`

In practice, you will want `repo_nspawn_plugins` instead.

The snapshots must already be in the container's image, and must have been
built by the `rpm_repo_snapshot()` target, and installed via
`install_rpm_repo_snapshot()`.

Also starts FBPKG proxy server if needed.
"""
import logging
import os
import textwrap
from contextlib import contextmanager, ExitStack
from dataclasses import dataclass
from io import BytesIO
from typing import Any, Dict, Generator, Iterable, List, Optional, Tuple

from antlir.artifacts_dir import find_repo_root

from antlir.bzl.proxy_server_config import proxy_server_config_t
from antlir.common import get_logger, pipe

from antlir.fbpkg.db.constants import MAIN_DB_PATH
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import _NspawnOpts, PopenArgs
from antlir.nspawn_in_subvol.netns_socket import create_sockets_inside_netns
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
    _PopenResult,
    _PostSetupPopenCtxMgr,
)

from antlir.nspawn_in_subvol.plugins import NspawnPlugin
from antlir.nspawn_in_subvol.plugins.launch_apt_proxy_server import (
    DEB_PROXY_SERVER_PORT,
    launch_apt_proxy_server_for_netns,
)
from antlir.nspawn_in_subvol.plugins.launch_proxy_server import (
    launch_proxy_server_for_netns,
    PROXY_SERVER_PORT,
)
from antlir.nspawn_in_subvol.plugins.launch_repo_servers import (
    launch_repo_servers_for_netns,
)
from antlir.subvol_utils import Subvol

log: logging.Logger = get_logger()

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
    ) -> Generator["_ContainerPidExfiltrator", Any, Any]:
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
    def exfiltrate_container_pid(self) -> Generator[int, Any, Any]:
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
) -> Generator[Tuple[_NspawnOpts, Any], Any, Any]:
    with _ContainerPidExfiltrator.new(
        # Below, we append FDs to `forward_fd`.  In the container, these
        # will map sequentially to `3 + len(opts.forward_fd)` and up.
        exfil_w_dest_fd=3 + len(list(opts.forward_fd)),
        ready_r_dest_fd=4 + len(list(opts.forward_fd)),
    ) as cpe:
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
    def __init__(
        self,
        serve_rpm_snapshots: Iterable[Path],
        proxy_server_config: Optional[proxy_server_config_t] = None,
        run_apt_proxy: bool = False,
    ) -> None:
        self._serve_rpm_snapshots = serve_rpm_snapshots
        self._run_proxy_server: bool = False
        self._fbpkg_db_path: Optional[Path] = None
        self._run_apt_proxy: bool = run_apt_proxy
        if proxy_server_config:
            self._run_proxy_server = True

            self._fbpkg_db_path = (
                find_repo_root(Path(os.getcwd())) / MAIN_DB_PATH
            )

    @staticmethod
    def _ns_sockets_needed(
        serve_rpm_snapshots: Iterable[Path], snap_subvol: Subvol
    ) -> Tuple[int, Dict[Path, int]]:
        socks_needed, socks_per_snapshot = 0, {}
        for snap_dir in serve_rpm_snapshots:
            with open(
                snap_subvol.path(snap_dir) / "ports-for-repo-server"
            ) as f:
                s_count = len({int(v) for v in f.read().split() if v})
                socks_needed += s_count
                socks_per_snapshot[snap_dir] = s_count

        return socks_needed, socks_per_snapshot

    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        subvol: Subvol,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> Generator[_NspawnSetup, Any, Any]:
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

        with _wrap_opts_with_container_pid_exfiltrator(opts) as (
            opts,
            cpe,
            # pyre-fixme[19]: Expected 2 positional arguments.
        ), setup_ctx(subvol, opts, popen_args) as setup:
            # pyre-fixme[16]: `RepoServers` has no attribute
            #  `_container_pid_exfiltrator`.
            self._container_pid_exfiltrator = cpe

            yield setup

    @contextmanager
    def wrap_post_setup_popen(
        self, post_setup_popen_ctx: _PostSetupPopenCtxMgr, setup: _NspawnSetup
    ) -> Generator[_PopenResult, Any, Any]:
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

            ns_count, sockets_per_snapshot = self._ns_sockets_needed(
                serve_rpm_snapshots, snap_subvol
            )

            if self._run_proxy_server:
                ns_count += 1

            if self._run_apt_proxy:
                ns_count += 1  # pragma: no cover

            ns_sockets_pool = create_sockets_inside_netns(
                container_pid, ns_count
            )
            log.debug(f"Created {ns_count} sockets in {container_pid} ns ")

            # To speed up startup, launch all the servers, and then await them.
            snap_to_servers = {}

            for snap_dir in serve_rpm_snapshots:
                snap_to_servers[snap_dir] = stack.enter_context(
                    launch_repo_servers_for_netns(
                        ns_sockets=ns_sockets_pool[
                            0 : sockets_per_snapshot[snap_dir]
                        ],
                        snapshot_dir=snap_subvol.path(snap_dir),
                        repo_server_bin=snap_subvol.path(
                            snap_dir / "repo-server"
                        ),
                    )
                )
                ns_sockets_pool = ns_sockets_pool[
                    sockets_per_snapshot[snap_dir] :
                ]

            if snap_to_servers:
                log.info(
                    "Started `repo-server` for snapshots (ports): "
                    + ", ".join(
                        f"""{snap.basename()} ({' '.join(
                            str(s.port) for s in servers
                        )})"""
                        for snap, servers in snap_to_servers.items()
                    )
                )

            if self._run_proxy_server:
                stack.enter_context(
                    launch_proxy_server_for_netns(
                        ns_socket=ns_sockets_pool.pop(),
                        fbpkg_db_path=self._fbpkg_db_path,
                    )
                )

                log.info(f"Started `proxy-server` on port {PROXY_SERVER_PORT}")
            if self._run_apt_proxy:  # pragma: no cover
                stack.enter_context(
                    launch_apt_proxy_server_for_netns(
                        ns_socket=ns_sockets_pool.pop(),
                        bucket_name="antlir_snapshots",
                        api_key="antlir_snapshots-key",
                    )
                )
                log.info(
                    f"Started `deb-proxy-server` on port {DEB_PROXY_SERVER_PORT}"
                )

            self._container_pid_exfiltrator.send_ready()
            yield popen_res
