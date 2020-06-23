#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
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
'''

from types import MappingProxyType
from typing import Iterable, Tuple

from fs_image.common import set_new_key
from fs_image.fs_utils import Path
from fs_image.subvol_utils import Subvol

from . import NspawnPlugin
from .repo_servers import repo_servers_nspawn_plugin
from .yum_dnf_versionlock import yum_dnf_versionlock_nspawn_plugin


def nspawn_rpm_plugins(
    subvol: Subvol,
    *,
    serve_rpm_snapshots: Iterable[Path],
    snapshots_and_versionlocks: Iterable[Tuple[Path, Path]] = None,
) -> Iterable[NspawnPlugin]:
    serve_rpm_snapshots = frozenset(
        # Canonicalize here and below to ensure that it doesn't matter if
        # snapshots are specified by symlink or by real location.
        subvol.canonicalize_path(p) for p in serve_rpm_snapshots
    )

    # Sanity-check the snapshot -> versionlock map
    s_to_vl = {}
    for s, vl in snapshots_and_versionlocks or ():
        s = subvol.canonicalize_path(s)
        assert s in serve_rpm_snapshots, (s, serve_rpm_snapshots)
        # Future: we should probably allow duplicates if the canonicalized
        # source and destination are both the same.
        set_new_key(s_to_vl, s, vl)
    snapshot_to_versionlock = MappingProxyType(s_to_vl)

    return (
        yum_dnf_versionlock_nspawn_plugin(snapshot_to_versionlock),
        repo_servers_nspawn_plugin(serve_rpm_snapshots),
    ) if serve_rpm_snapshots else ()
