#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Wrap `popen_{non,}_booted_nspawn` with `inject_yum_dnf_versionlock` to
populate the `versionlock.list` files inside the specified repo snapshots
inside the container.

For the `run_*` functions, add this to your `popen_wrappers`:
  `functools.partial(inject_yum_dnf_versionlock, snapshot_to_versionlock)`

To provide `versionlock.list` files in the container, this parses our own
"version lock" format documented in `args.py` (or `--help` on the CLI),
generates the `yum`- or `dnf`-specific variant of the format, and
bind-mounts them into the snapshots that already exist in the container's
image.  This allows us to change version selections on a more frequent
cadence than we change repo snapshots.
'''
import functools

from contextlib import contextmanager, ExitStack
from typing import Dict, Mapping, Tuple

from fs_image.common import get_file_logger, set_new_key
from fs_image.fs_utils import create_ro, Path, temp_dir
from subvol_utils import Subvol

from .args import _NspawnOpts, PopenArgs
from .common import _PopenCtxMgr

log = get_file_logger(__file__)


@contextmanager
def _prepare_versionlock_lists(
    subvol: Subvol, snapshot_dir: Path, list_path: Path
) -> Dict[str, Tuple[str, int]]:
    '''
    Returns a map of "in-snapshot path" -> "tempfile with its contents",
    with the intention that the tempfile in the value will be a read-only
    bind-mount over the path in the key.
    '''
    # `dnf` and `yum` expect different formats, so we parse our own.
    with open(list_path) as rf:
        envras = [l.split('\t') for l in rf]
    templates = {b'yum': '{e}:{n}-{v}-{r}.{a}', b'dnf': '{n}-{e}:{v}-{r}.{a}'}
    dest_to_src_and_size = {}
    with temp_dir() as d:
        # Only bind-mount lists for those binaries that exist in the snapshot.
        for prog in (subvol.path(snapshot_dir) / 'bin').listdir():
            template = templates.get(prog)
            # For now, `bin` has <= 2 binaries, but this can be relaxed later:
            assert template, prog
            src = d / (prog + b'-versionlock.list')
            with create_ro(src, 'w') as wf:
                for e, n, v, r, a in envras:
                    wf.write(template.format(e=e, n=n, v=v, r=r, a=a))
            set_new_key(
                dest_to_src_and_size,
                # This path convention must match how `write_yum_dnf_conf.py`
                # and `rpm_repo_snapshot.bzl` set up their output.
                snapshot_dir / f'etc/{prog}/plugins/versionlock.list',
                (src, len(envras))
            )
        yield dest_to_src_and_size


def inject_yum_dnf_versionlock(
    snapshot_to_versionlock: Mapping[Path, Path], popen: _PopenCtxMgr,
) -> _PopenCtxMgr:
    'Wraps `popen_booted_nspawn` or `popen_non_booted_nspawn`.'

    @functools.wraps(popen)
    @contextmanager
    def wrapped_popen(opts: _NspawnOpts, popen_args: PopenArgs):
        with ExitStack() as stack:
            dest_to_src = {}
            for snapshot, versionlock in snapshot_to_versionlock.items():
                for dest, (src, vl_size) in stack.enter_context(
                    _prepare_versionlock_lists(
                        # Same note as in `inject_repo_servers.py` regarding
                        # the usage of the pre-snapshot subvolume.
                        opts.layer, snapshot, versionlock,
                    )
                ).items():
                    log.info(f'Locking {vl_size} RPM versions via {dest}')
                    set_new_key(dest_to_src, dest, src)
            yield stack.enter_context(popen(
                opts._replace(
                    bindmount_ro=(*opts.bindmount_ro, *(
                        (s, d) for d, s in dest_to_src.items()
                    )),
                ),
                popen_args,
            ))

    return wrapped_popen
