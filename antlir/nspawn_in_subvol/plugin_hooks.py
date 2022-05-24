#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"The core logic of how plugins integrate with `popen_nspawn`"

import functools
import subprocess
from contextlib import contextmanager
from typing import Callable, ContextManager, Iterable, Tuple

from antlir.subvol_utils import Subvol

from .args import _NspawnOpts, PopenArgs
from .cmd import _nspawn_setup, _nspawn_subvol_setup, _NspawnSetup
from .plugins import NspawnPlugin


# pyre-fixme[24]: Generic type `subprocess.Popen` expects 1 type parameter.
_PopenResult = Tuple[subprocess.Popen, subprocess.Popen]
_SetupSubvolCtxMgr = Callable[[_NspawnOpts], ContextManager[Subvol]]
_NspawnSetupCtxMgr = Callable[
    [_NspawnOpts, PopenArgs], ContextManager[_NspawnSetup]
]
_PostSetupPopenCtxMgr = Callable[[_NspawnSetup], ContextManager[_PopenResult]]


@contextmanager
def _setup_subvol(opts: _NspawnOpts) -> Iterable[Subvol]:
    # pyre-fixme[16]: `Subvol` has no attribute `__enter__`.
    with _nspawn_subvol_setup(opts) as subvol:
        yield subvol


@contextmanager
def _setup(
    subvol: Subvol, opts: _NspawnOpts, popen_args: PopenArgs
) -> Iterable[_NspawnSetup]:
    # pyre-fixme[16]: `_NspawnSetup` has no attribute `__enter__`.
    with _nspawn_setup(subvol, opts, popen_args) as setup:
        yield setup


@contextmanager
def _popen_plugin_driver(
    opts: _NspawnOpts,
    popen_args: PopenArgs,
    post_setup_popen: _PostSetupPopenCtxMgr,
    plugins: Iterable[NspawnPlugin],
) -> _PopenResult:
    # Apply the plugins
    setup = _setup
    setup_subvol = _setup_subvol
    for p in plugins:
        if p.wrap_setup_subvol is not None:
            setup_subvol = functools.partial(p.wrap_setup_subvol, setup_subvol)
        if p.wrap_setup is not None:
            setup = functools.partial(p.wrap_setup, setup)
        if p.wrap_post_setup_popen is not None:
            post_setup_popen = functools.partial(
                p.wrap_post_setup_popen, post_setup_popen
            )

    with setup_subvol(opts) as subvol, setup(
        subvol, opts, popen_args
    ) as setup, post_setup_popen(setup) as popen_res:
        # pyre-fixme[7]: Expected `Tuple[subprocess.Popen[typing.Any],
        # subprocess.Popen[typing.Any]]` but got `Generator[typing.Any,
        # None, None]`.
        yield popen_res
