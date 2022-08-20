#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import pwd
import shlex
import tempfile
from typing import Any, Iterable, Optional

from antlir.bzl.image.feature.apt import action_t, apt_action_item_t
from antlir.compiler.items.common import ImageItem, LayerOpts, PhaseOrder
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import (
    _new_nspawn_debug_only_not_for_prod_opts,
    new_nspawn_opts,
    NspawnPluginArgs,
    PopenArgs,
)
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.launch_apt_proxy_server import (
    DEB_PROXY_SERVER_PORT,
)
from antlir.nspawn_in_subvol.plugins.repo_plugins import repo_nspawn_plugins
from antlir.subvol_utils import Subvol


def _apt_get_cmd(args: Iterable[str], action) -> Optional[str]:
    if action == action_t.INSTALL:
        return (
            shlex.join(["apt-get", "update"])
            + " && "
            + shlex.join(
                ["apt-get", "-y", "--no-install-recommends", "install"]
                + list(args)
            )
            + " && "
            + shlex.join(["apt-get", "clean"])
        )
    if action == action_t.REMOVE_IF_EXISTS:
        return (
            shlex.join(["apt-get", "-y", "remove", "--purge"] + list(args))
            + " && "
            + shlex.join(["apt-get", "-y", "autoremove", "--purge"])
        )


class AptActionItems(apt_action_item_t, ImageItem):
    def __init__(self, *args: Any, **kwargs: Any):
        apt_action_item_t.__init__(self, *args, **kwargs)
        ImageItem.__init__(self, from_target=kwargs.get("from_target"))

    def phase_order(self):
        return {
            "action_t.INSTALL": PhaseOrder.APT_INSTALL,
            "action_t.REMOVE_IF_EXISTS": PhaseOrder.APT_REMOVE,
        }[str(self.action)]

    @classmethod
    def get_phase_builder(
        cls,
        items: Iterable["AptActionItems"],
        layer_opts: LayerOpts,
    ):
        apt_snapshot = layer_opts.apt_repo_snapshot

        packages = []
        for item in items:
            packages += item.package_names

        # assert all the packages belong to the same action item
        assert len({str(item.action) for item in items}) == 1

        apt_cmd = _apt_get_cmd(packages, item.action)

        def builder(subvol: Subvol) -> None:
            layer_opts.requires_build_appliance()
            sources_list = Path.join(tempfile.mkdtemp(), "sources.list")
            with open(sources_list, "w+") as f:
                for snapshot in apt_snapshot:
                    f.write(
                        f"deb [trusted=yes] http://127.0.0.1:{DEB_PROXY_SERVER_PORT}/ {snapshot}"
                    )

            opts = new_nspawn_opts(
                cmd=[
                    "/bin/bash",
                    "-cue",
                    f"{apt_cmd}",
                ],
                layer=subvol,
                snapshot=False,
                bindmount_ro=[(sources_list, "/etc/apt/sources.list")],
                user=pwd.getpwnam("root"),
                debug_only_opts=_new_nspawn_debug_only_not_for_prod_opts(
                    # This needs to be set to public so that the apt proxy server
                    # launched by the outer BA container is reachable from the
                    # below nspawn.
                    private_network=False,
                ),
            )
            run_nspawn(
                opts,
                PopenArgs(),
                plugins=repo_nspawn_plugins(
                    opts=opts,
                    plugin_args=NspawnPluginArgs(
                        shadow_proxied_binaries=False,
                        run_apt_proxy=False,
                    ),
                ),
            )

        return builder
