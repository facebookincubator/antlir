#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The RPM-related plugins need to be composed in a specific way with one
another, and with the plugin that handles shadowing proxied binaries.

This here is the easiest implementation, which is simple at the cost of
tight coupling.

TECH DEBT ALERT: As we add support for other plugins and package managers,
this will no longer be adequate.  Let's be careful not to make this a
kitchen-sink method, and instead devise a more flexible means of composing
plugins. Specifically:

  - The repo server and versionlock plugins can be tightly coupled with no
    harm to maintainability (i.e. the implementations may stay the same, but
    could be hidden behind a tiny common wrapper like this one)

  - To combine the "shadowed proxied binaries" plugin with the package
    manager plugin(s), one would need a declaration layer for plugins,
    explicit or implicit sequencing for who gets to declare first, and an
    evaluation layer that consumes the declarations and outputs an
    `Iterable[NspawnPlugins]`.  To make this more specific, this would
    likely involve giving a true class interface to the plugins, and using
    that to encode the desired dataflow.
"""

from types import MappingProxyType
from typing import Iterable

from antlir.common import set_new_key
from antlir.fs_utils import RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
from antlir.nspawn_in_subvol.args import NspawnPluginArgs, _NspawnOpts

from . import NspawnPlugin
from .repo_servers import RepoServers
from .shadow_paths import ShadowPaths
from .yum_dnf_versionlock import YumDnfVersionlock


def rpm_nspawn_plugins(
    *, opts: _NspawnOpts, plugin_args: NspawnPluginArgs
) -> Iterable[NspawnPlugin]:
    serve_rpm_snapshots = set(plugin_args.serve_rpm_snapshots)
    shadow_paths = [*plugin_args.shadow_paths]

    # Shadow RPM installers by default, when running as "root".  It is ugly
    # to condition this on "root", but in practice, it is a significant
    # costs savings for non-root runs -- no need to start repo servers and
    # shadow bind mounts that (`sudo` aside) would not get used.
    default_snapshot_dir = opts.layer.path(
        RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
    )
    if (
        plugin_args.shadow_proxied_binaries
        and opts.user.pw_name == "root"
        and default_snapshot_dir.exists()
    ):
        # This can run as non-root since `_set_up_rpm_repo_snapshots` makes
        # this a world-readable directory.
        for prog_name in default_snapshot_dir.listdir():
            # Here, we need container, not host paths
            snapshot_dir = RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / prog_name
            serve_rpm_snapshots.add(snapshot_dir)
            shadow_paths.append(
                (prog_name, snapshot_dir / prog_name / "bin" / prog_name)
            )

    # Canonicalize paths here and below to ensure that it doesn't matter if
    # snapshots are specified by symlink or by real location.
    serve_rpm_snapshots = frozenset(
        opts.layer.canonicalize_path(p) for p in serve_rpm_snapshots
    )

    # Sanity-check the snapshot -> versionlock map
    s_to_vl = {}
    for s, vl in plugin_args.snapshots_and_versionlocks or ():
        s = opts.layer.canonicalize_path(s)
        assert s in serve_rpm_snapshots, (s, serve_rpm_snapshots)
        # Future: we should probably allow duplicates if the canonicalized
        # source and destination are both the same.
        set_new_key(s_to_vl, s, vl)
    snapshot_to_versionlock = MappingProxyType(s_to_vl)

    return (
        # This handles `ShadowPaths` even though it's not RPM-specific
        # because the two integrate -- a stacked diff will add a default
        # behavior to shadow the OS `yum` / `dnf` binaries with wrappers
        # that talk to our repo servers in `nspawn_in_subvol` containers.
        *([ShadowPaths(shadow_paths)] if shadow_paths else []),
        *(
            [
                *(
                    [YumDnfVersionlock(snapshot_to_versionlock)]
                    if snapshot_to_versionlock
                    else []
                ),
                RepoServers(serve_rpm_snapshots),
            ]
            if serve_rpm_snapshots
            else ()
        ),
    )
