#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The Repo (RPM and others)-related plugins need to be composed in a specific way
with one another, and with the plugin that handles shadowing proxied binaries.

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

from typing import Iterable

from antlir.fs_utils import ANTLIR_DIR, RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
from antlir.nspawn_in_subvol.args import (
    _NspawnOpts,
    AttachAntlirDirMode,
    NspawnPluginArgs,
)
from antlir.nspawn_in_subvol.common import AttachAntlirDirError

from . import NspawnPlugin
from .attach_antlir_dir import AttachAntlirDir
from .repo_servers import RepoServers
from .shadow_paths import ShadowPaths
from .yum_dnf_versionlock import YumDnfVersionlock


def _get_snapshot_dir(opts: _NspawnOpts, plugin_args: NspawnPluginArgs):
    # Shadow RPM installers by default, when running as "root".  It is ugly
    # to condition this on "root", but in practice, it is a significant
    # costs savings for non-root runs -- no need to start repo servers and
    # shadow bind mounts that (`sudo` aside) would not get used.
    if (
        plugin_args.attach_antlir_dir != AttachAntlirDirMode.OFF
        and not opts.layer.path(ANTLIR_DIR).exists()
        and opts.subvolume_on_disk
        # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute
        # `build_appliance_path`.
        and opts.subvolume_on_disk.build_appliance_path
    ):
        return (
            opts.subvolume_on_disk.build_appliance_path
            / RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR.strip_leading_slashes()
        )

    if plugin_args.attach_antlir_dir == AttachAntlirDirMode.EXPLICIT_ON:
        raise AttachAntlirDirError(
            "ERROR: Could not attach /__antlir__ dir. Please "
            "check to make sure that you do not have an existing antlir "
            "directory in your image, and that the image has a "
            "discoverable build appliance (usually through its flavor)."
        )

    return opts.layer.path(RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR)


def repo_nspawn_plugins(
    *, opts: _NspawnOpts, plugin_args: NspawnPluginArgs
) -> Iterable[NspawnPlugin]:
    serve_rpm_snapshots = set(plugin_args.serve_rpm_snapshots)
    shadow_paths = [*plugin_args.shadow_paths]

    default_snapshot_dir = _get_snapshot_dir(opts, plugin_args)
    shadow_paths_allow_unmatched = []

    if (
        plugin_args.shadow_proxied_binaries
        and opts.user.pw_name == "root"
        and default_snapshot_dir.exists()
    ):
        # This can run as non-root since `_set_up_rpm_repo_snapshots` makes
        # this a world-readable directory.
        for prog_name in sorted(default_snapshot_dir.listdir()):
            # Here, we need container, not host paths
            snapshot_dir = RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR / prog_name
            serve_rpm_snapshots.add(snapshot_dir)
            shadow_paths.append(
                (prog_name, snapshot_dir / prog_name / "bin" / prog_name)
            )
            if plugin_args.attach_antlir_dir == AttachAntlirDirMode.DEFAULT_ON:
                shadow_paths_allow_unmatched.append(prog_name)

    # pyre-fixme[60]: Concatenation not yet support for multiple
    # variadic tuples: `*[...
    return (
        *(
            [AttachAntlirDir()]
            # In default-on mode, do NOT try to attach the BA's `ANTLIR_DIR`
            # when the layer itself also has a `ANTLIR_DIR` -- first, this
            # would fail an assert in `AttachAntlirDir`, and second the
            # user likely wants to use the layer's `/__antlir__` anyway.
            if (
                plugin_args.attach_antlir_dir == AttachAntlirDirMode.DEFAULT_ON
                and not opts.layer.path(ANTLIR_DIR).exists()
            )
            or plugin_args.attach_antlir_dir == AttachAntlirDirMode.EXPLICIT_ON
            else []
        ),
        # This handles `ShadowPaths` even though it's not
        # RPM-specific because the two integrate -- a stacked diff
        # will add a default behavior to shadow the OS
        # `yum` / `dnf` binaries with wrappers that talk to our
        # repo servers in `nspawn_in_subvol` containers.
        *(
            [
                ShadowPaths(
                    shadow_paths,
                    shadow_paths_allow_unmatched,
                )
            ]
            if shadow_paths
            else []
        ),
        *(
            [
                *(
                    [
                        YumDnfVersionlock(
                            plugin_args.snapshots_and_versionlocks,
                            serve_rpm_snapshots,
                        )
                    ]
                    if plugin_args.snapshots_and_versionlocks
                    else []
                ),
                RepoServers(
                    serve_rpm_snapshots,
                    plugin_args.proxy_server_config,
                ),
            ]
            if serve_rpm_snapshots or plugin_args.proxy_server_config
            else ()
        ),
    )
