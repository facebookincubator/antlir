#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
XXX docs from wiki
"""
from contextlib import contextmanager
from typing import Any, Callable, FrozenSet, Iterable, Mapping, Union

from antlir.fs_utils import Path

from antlir.rpm.allowed_versions.envra import SortableEVRA
from antlir.rpm.pluggable import Pluggable


class VersionPolicy(Pluggable):
    pass


# Import FB-specific implementations if available.  This must come after
# VersionPolicy to resolve the circular dependency.
try:
    from antlir.rpm.allowed_versions.facebook import (  # noqa: F401
        version_policy as _fb_version_policy,
    )
except ImportError:  # pragma: no cover
    pass


def _deserialize_evra(arch: str, evr: Union[str, Mapping[str, Any]]) -> SortableEVRA:
    def evr_to_evra(e, v, r):
        # Fixme: this EVRA is implemented as an ENVRA
        return SortableEVRA(
            name=None,
            epoch=e,
            version=v,
            release=r,
            arch=None if arch == "*" else arch,
        )

    if isinstance(evr, str):
        # We support `V-R` (and not yet `E:V-R`) because this is by far the
        # most common short-hand in the RPM ecosystem.  Most users won't
        # even know about `epoch`, and would have to look up how to get it.
        # Besides ambiguity, this does not cost us much since we must
        # resolve epochs anyway in `update_allowed_versions.py`.
        assert ":" not in evr, "We can add `E:V-R` parsing if requested."
        ver_rel = evr.split("-")
        if len(ver_rel) != 2:
            raise RuntimeError(f"Expected version-release pair, got {ver_rel}")
        return evr_to_evra(None, *ver_rel)

    if isinstance(evr, dict):
        evr_copy = evr.copy()  # We're about to mutate it
        try:
            # For now, this doesn't allow epoch to be a wildcard.  Use
            # the string form to specify that.
            evra = evr_to_evra(
                int(evr_copy.pop("epoch")),
                str(evr_copy.pop("version")),
                str(evr_copy.pop("release")),
            )
        except Exception:
            raise RuntimeError(f"Parsing EVR spec {evr}")
        if evr_copy:
            raise RuntimeError(f"Unknown keys in version spec: {evr_copy}")
        return evra

    raise RuntimeError(f"Bad EVR spec {evr}")


class ManualVersionPolicy(VersionPolicy, plugin_kind="manual"):
    def __init__(self) -> None:
        pass  # This tells `add_argparse_arg` we take no args.

    async def snapshot(self, snapshot_dir: Path) -> None:
        '"manual" version policies do not need to snapshot anything'

    @contextmanager
    def load_config_fn(
        self, snapshot_dir: Path, _flavor: str
    ) -> Iterable[Callable[..., FrozenSet[SortableEVRA]]]:
        """
        Returns a config-loader, which parses a user-specified version set
        in the form described
        [here](/docs/concepts/rpms/version-selection/#version-policy).
        """

        def load_config(*, packages, versions):
            # Not worried about mutable aliases since it's a brand-new dict.
            return frozenset(
                _deserialize_evra(arch, evr)
                for arch, evrs in versions.items()
                for evr in evrs
            )

        yield load_config
