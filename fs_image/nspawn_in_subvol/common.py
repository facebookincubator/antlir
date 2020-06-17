#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'No externally useful functions here.  Read the `run.py` docblock instead.'
import functools
import subprocess

from typing import Any, Callable, Iterable, NamedTuple

from fs_image.fs_utils import Path

SHADOWED_PATHS_ROOT = Path('__fs_image__/shadowed')

# This determines which binaries we shadow.  Our runtimes are expected to
# ensure that this is the PATH for the user command in the container.
#
# For now, the non-booted case implicitly uses the `systemd-nspawn` default
# `PATH`, so if that changes our test will fail.  That test failure in time
# will be an opportunity to decide whether to set our own, or follow.
DEFAULT_SEARCH_PATHS = (Path(p) for p in (
    '/usr/local/sbin',
    '/usr/local/bin',
    '/usr/sbin',
    '/usr/bin',
    '/sbin',
    '/bin',
))
DEFAULT_PATH_ENV = b':'.join(DEFAULT_SEARCH_PATHS)

_PopenCtxMgr = Any  # Quacks like `popen_{non_,}booted_nspawn`


class NspawnWrapper(NamedTuple):
    popen: Callable[[_PopenCtxMgr], _PopenCtxMgr] = None


def apply_wrappers_to_popen(
    wrappers: Iterable[NspawnWrapper], popen: _PopenCtxMgr,
) -> _PopenCtxMgr:
    return functools.reduce(
        (lambda x, f: f(x)),
        (w.popen for w in wrappers if w.popen is not None),
        popen,
    )


def nspawn_version():
    '''
    We now care about the version of nspawn we are running.  The output of
    systemd-nspawn --version looks like:

    ```
    systemd 242 (v242-2.fb1)
    +PAM +AUDIT +SELINUX +IMA ...
    ```
    So we can get the major version as the second token of the first line.
    We hope that the output of systemd-nspawn --version is stable enough
    to keep parsing it like this.
    '''
    return int(subprocess.check_output([
        'systemd-nspawn', '--version']).split()[1])
