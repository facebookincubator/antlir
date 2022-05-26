#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
XXX docs from wiki
"""
from contextlib import contextmanager
from typing import Callable, Iterable

from antlir.fs_utils import Path
from antlir.rpm.pluggable import Pluggable


class PackageGroup(Pluggable):
    pass


# Import FB-specific implementations if available.  This must come after
# PackageGroup to resolve the circular dependency.
try:
    from .facebook import package_group as _fb_package_group  # noqa: F401
except ImportError:  # pragma: no cover
    pass


# XXX: Future: access the RPM snapshot SQLite DB here to allow defining
# package groups by source RPM name (for each src.rpm name mentioned, group
# all the packages that are built by any of the src.rpm versions -- this
# over-matches a bit, but is probably fine.


class ManualPackageGroup(PackageGroup, plugin_kind="manual"):
    def __init__(self) -> None:
        pass  # This tells `add_argparse_arg` we take no args.

    async def snapshot(self, snapshot_dir: Path) -> None:
        '"manual" package groups do not need to snapshot anything'

    @contextmanager
    def load_config_fn(
        self, snapshot_dir: Path, _flavor: str
    ) -> Iterable[Callable[..., Iterable[str]]]:
        """
        The context function takes config kwargs: names = ["a", "b", "c"]
        It returns the package names unchanged.
        """

        def load_config(*, names):
            return tuple(names)  # Tuple to avoid mutable alias bugs

        yield load_config
