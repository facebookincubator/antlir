#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Populate the `versionlock.list` files inside the specified repo snapshots
inside the container by adding this to `plugins` kwarg of the `run_*` or
`popen_*` functions: `YumDnfVersionlock(snapshot_to_versionlock)`

In practice, you will want `repo_nspawn_plugins` instead.

To provide `versionlock.list` files in the container, this parses our own
"version lock" format documented in `args.py` (or `--help` on the CLI),
generates the `yum`- or `dnf`-specific variant of the format, and
bind-mounts them into the snapshots that already exist in the container's
image.  This allows us to change version selections on a more frequent
cadence than we change repo snapshots.
"""
from contextlib import contextmanager, ExitStack
from types import MappingProxyType
from typing import Dict, Iterable, Tuple

from antlir.common import get_logger, set_new_key
from antlir.fs_utils import create_ro, Path, temp_dir
from antlir.nspawn_in_subvol.args import _NspawnOpts, PopenArgs
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
    _SetupSubvolCtxMgr,
)

from antlir.nspawn_in_subvol.plugins import NspawnPlugin
from antlir.subvol_utils import Subvol


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
        # pyre-fixme[7]: Expected `Dict[str, Tuple[str, int]]` but got
        #  `Generator[Dict[typing.Any, typing.Any], None, None]`.
        yield dest_to_src_and_size


class YumDnfVersionlock(NspawnPlugin):
    def __init__(
        self,
        snapshots_and_versionlocks: Iterable[Tuple[Path, Path]],
        serve_rpm_snapshots: Iterable[Path],
    ) -> None:
        self._snapshots_and_versionlocks = snapshots_and_versionlocks
        self._serve_rpm_snapshots = serve_rpm_snapshots

    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        subvol: Subvol,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> _NspawnSetup:

        # Sanity-check the snapshot -> versionlock map
        s_to_vl = {}
        serve_rpm_snapshots = {
            subvol.canonicalize_path(s) for s in self._serve_rpm_snapshots
        }
        for s, vl in self._snapshots_and_versionlocks or ():
            s = subvol.canonicalize_path(s)
            assert s in serve_rpm_snapshots, (
                s,
                serve_rpm_snapshots,
            )
            # Future: we should probably allow duplicates if the canonicalized
            # source and destination are both the same.
            set_new_key(s_to_vl, s, vl)
        snapshot_to_versionlock = MappingProxyType(s_to_vl)

        with ExitStack() as stack:
            dest_to_src = {}
            for snapshot, versionlock in snapshot_to_versionlock.items():
                for dest, (src, vl_size) in stack.enter_context(
                    # pyre-fixme[6]: Expected
                    # `ContextManager[Variable[contextlib._T]]` for 1st param
                    # but got `Dict[str, Tuple[str, int]]`.
                    _prepare_versionlock_lists(
                        subvol,
                        snapshot,
                        versionlock,
                    )
                ).items():
                    log.info(f"Locking {vl_size} RPM versions via {dest}")
                    set_new_key(dest_to_src, dest, src)
            # pyre-fixme[7]: Expected `_NspawnSetup` but got
            #  `Generator[antlir.nspawn_in_subvol.cmd._NspawnSetup, None,
            #  None]`.
            yield stack.enter_context(
                # pyre-fixme[19]: Expected 2 positional arguments.
                setup_ctx(
                    subvol,
                    opts._replace(
                        # pyre-fixme[60]: Concatenation not yet support for multiple
                        #  variadic tuples: `*opts.bindmount_ro, *comprehension((s, d)
                        #  for generators(generator((d, s) in dest_to_src.items() if
                        #  )))`.
                        bindmount_ro=(
                            *opts.bindmount_ro,
                            *((s, d) for d, s in dest_to_src.items()),
                        )
                    ),
                    popen_args,
                )
            )
