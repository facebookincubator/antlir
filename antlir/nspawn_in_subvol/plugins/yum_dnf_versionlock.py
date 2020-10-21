#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Populate the `versionlock.list` files inside the specified repo snapshots
inside the container by adding this to `plugins` kwarg of the `run_*` or
`popen_*` functions: `YumDnfVersionlock(snapshot_to_versionlock)`

In practice, you will want `rpm_nspawn_plugins` instead.

To provide `versionlock.list` files in the container, this parses our own
"version lock" format documented in `args.py` (or `--help` on the CLI),
generates the `yum`- or `dnf`-specific variant of the format, and
bind-mounts them into the snapshots that already exist in the container's
image.  This allows us to change version selections on a more frequent
cadence than we change repo snapshots.
"""
from contextlib import ExitStack, contextmanager
from typing import Dict, Mapping, Tuple

from antlir.common import get_logger, set_new_key
from antlir.fs_utils import Path, create_ro, temp_dir
from antlir.nspawn_in_subvol.args import PopenArgs, _NspawnOpts
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
)
from antlir.subvol_utils import Subvol

from . import NspawnPlugin


log = get_logger()


@contextmanager
def _prepare_versionlock_lists(
    subvol: Subvol, snapshot_dir: Path, list_path: Path
) -> Dict[str, Tuple[str, int]]:
    """
    Returns a map of "in-snapshot path" -> "tempfile with its contents",
    with the intention that the tempfile in the value will be a read-only
    bind-mount over the path in the key.
    """
    # `dnf` and `yum` expect different formats, so we parse our own.
    with open(list_path) as rf:
        envra_set = {tuple(l.split("\t")) for l in rf}
    templates = {"yum": "{e}:{n}-{v}-{r}.{a}", "dnf": "{n}-{e}:{v}-{r}.{a}"}
    dest_to_src_and_size = {}
    with temp_dir() as d:
        # Only bind-mount lists for those binaries that exist in the snapshot.
        for prog in {
            f"{p}" for p in (subvol.path(snapshot_dir)).listdir()
        } & set(templates.keys()):
            template = templates[prog]
            src = d / (prog + "-versionlock.list")
            with create_ro(src, "w") as wf:
                for e, n, v, r, a in envra_set:
                    wf.write(template.format(e=e, n=n, v=v, r=r, a=a))
            set_new_key(
                dest_to_src_and_size,
                # This path convention must match how `write_yum_dnf_conf.py`
                # and `rpm_repo_snapshot.bzl` set up their output.
                snapshot_dir / f"{prog}/etc/{prog}/plugins/versionlock.list",
                (src, len(envra_set)),
            )
        yield dest_to_src_and_size


class YumDnfVersionlock(NspawnPlugin):
    def __init__(self, snapshot_to_versionlock: Mapping[Path, Path]):
        self._snapshot_to_versionlock = snapshot_to_versionlock

    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> _NspawnSetup:
        with ExitStack() as stack:
            dest_to_src = {}
            for snapshot, versionlock in self._snapshot_to_versionlock.items():
                for dest, (src, vl_size) in stack.enter_context(
                    _prepare_versionlock_lists(
                        # Same note as in `repo_servers.py` regarding the
                        # usage of the pre-snapshot subvolume.
                        opts.layer,
                        snapshot,
                        versionlock,
                    )
                ).items():
                    log.info(f"Locking {vl_size} RPM versions via {dest}")
                    set_new_key(dest_to_src, dest, src)
            yield stack.enter_context(
                setup_ctx(
                    opts._replace(
                        bindmount_ro=(
                            *opts.bindmount_ro,
                            *((s, d) for d, s in dest_to_src.items()),
                        )
                    ),
                    popen_args,
                )
            )
