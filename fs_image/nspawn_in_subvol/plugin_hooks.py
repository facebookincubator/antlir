#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'The core logic of how plugins integrate with `popen_{non,}_booted_nspawn`'

import functools
import subprocess

from contextlib import contextmanager
from typing import Callable, ContextManager, Iterable, Tuple, Union

from .args import _NspawnOpts, PopenArgs
from .cmd import _NspawnSetup, _nspawn_setup
from .plugins import NspawnPlugin

_PopenResult = Union[
    subprocess.Popen,  # non-booted
    Tuple[subprocess.Popen, subprocess.Popen],  # booted
]
_NspawnSetupCtxMgr = Callable[
    [_NspawnOpts, PopenArgs], ContextManager[_NspawnSetup]
]
_PostSetupPopenCtxMgr = Callable[[_NspawnSetup], ContextManager[_PopenResult]]


@contextmanager
def _setup(opts: _NspawnOpts, popen_args: PopenArgs) -> Iterable[_NspawnSetup]:
    with _nspawn_setup(opts, popen_args) as setup:
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
    for p in plugins:
        if p.wrap_setup is not None:
            setup = functools.partial(p.wrap_setup, setup)
        if p.wrap_post_setup_popen is not None:
            post_setup_popen = functools.partial(
                p.wrap_post_setup_popen, post_setup_popen,
            )
    with setup(opts, popen_args) as setup:
        with post_setup_popen(setup) as popen_res:
            yield popen_res
