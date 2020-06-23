#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools

from typing import Any, Callable, Iterable, NamedTuple, Optional

_OuterPopenCtxMgr = Any  # Quacks like `_outer_popen_{non_,}booted_nspawn`


class NspawnPlugin(NamedTuple):
    '''
    These go in the `plugins` field passed to `run_*` and `popen_` functions.
    See other `*.py` files in this directory for concrete examples.
    '''
    popen: Optional[Callable[[_OuterPopenCtxMgr], _OuterPopenCtxMgr]] = None


def apply_plugins_to_popen(
    plugins: Iterable[NspawnPlugin], popen: _OuterPopenCtxMgr,
) -> _OuterPopenCtxMgr:
    return functools.reduce(
        (lambda x, f: f(x)),
        (w.popen for w in plugins if w.popen is not None),
        popen,
    )
