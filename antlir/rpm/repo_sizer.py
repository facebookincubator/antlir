#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Repos in `yum.conf` can (and do) share Repodata and Rpm objects, so the best
estimate of their total space usage requires counting each object only once.

`RepoDownloader` feeds the requisite information to this visitor. This
implements the `RepoObjectVisitor` interface, described below.
"""
from collections import defaultdict
from typing import Any, Dict, NamedTuple, Union

from antlir.unionfind import UnionFind

from .common import Checksum
from .repo_objects import Repodata, RepoMetadata, Rpm


# Type of object provided to the repo downloader as a visitor, which must
# implement the following methods:
#   def visit_repomd(self, repomd: RepoMetadata) -> None:
#   def visit_repodata(self, repodata: Repodata) -> None:
#   def visit_rpm(self, rpm: Rpm) -> None:
#
# Future: Define a concrete interface for this.
# pyre-fixme[33]: Aliased annotation cannot be `Any`.
RepoObjectVisitor = Any


class Synonyms(NamedTuple):
    checksum_size: Dict[Checksum, int]
    # `checksums` is a disjoint-set data structure which contains sets of
    # checksums and tracks equivalencies (when two different checksums point to
    # the same repo object). Note that the `representative` in the UnionFind is
    # not necessarily the `canonical_checksum`, instead it's just a random
    # checksum representing the set to which it belongs
    checksums: UnionFind


class _ObjectCounter:
    def __init__(self) -> None:
        # An alternative to keying everything on checksum would be to use
        # keys like `checksum` for `Repodata` and `nevra` for `Rpm`.
        # Uniformly using checksums gracefully handles `MutableRpmError`,
        # and keeps this code generic.
        self._synonyms = Synonyms({}, UnionFind())

    # pyre-fixme[2]: Parameter must be annotated.
    def _set_size(self, chk, obj_size: int) -> None:
        """Helper to add a key into `checksum_size` while also performing
        a sanity check to ensure that, if the checksum already existed in the
        map, the size is the same.
        """
        size = self._synonyms.checksum_size.setdefault(chk, obj_size)
        assert (
            size == obj_size
        ), f"{chk} has prior size {size}, while the new size is {obj_size}"

    def add_repo_obj(self, obj: Union[Rpm, Repodata, RepoMetadata]) -> None:
        self._set_size(obj.checksum, obj.size)
        if obj.best_checksum() != obj.checksum:
            self._set_size(obj.best_checksum(), obj.size)
            self._synonyms.checksums.union(obj.best_checksum(), obj.checksum)
        else:
            self._synonyms.checksums.add(obj.checksum)

    def total_size(self) -> int:
        rep_sizes = {}
        for chk, rep in self._synonyms.checksums.items():
            # Ensure all checksums considered synonyms have the same size
            chk_size = self._synonyms.checksum_size[chk]
            rep_size = self._synonyms.checksum_size[rep]
            assert chk_size == rep_size, (chk, chk_size, rep, rep_size)
            rep_sizes[rep] = rep_size
        return sum(rep_sizes.values())

    def __iadd__(self, other: "_ObjectCounter") -> "_ObjectCounter":
        for chk, rep in other._synonyms.checksums.items():
            self._set_size(chk, other._synonyms.checksum_size[chk])
            self._set_size(rep, other._synonyms.checksum_size[rep])
            self._synonyms.checksums.union(rep, chk)
        return self


class RepoSizer:
    def __init__(self) -> None:
        # Count each type of objects separately
        # pyre-fixme[4]: Attribute must be annotated.
        self._type_to_counter = defaultdict(_ObjectCounter)

    def __iadd__(self, other: "RepoSizer") -> "RepoSizer":
        for typ, ctr in other._type_to_counter.items():
            self._type_to_counter[typ] += ctr
        return self

    # pyre-fixme[2]: Parameter annotation cannot be `Any`.
    def _add_object(self, obj: Any) -> None:
        self._type_to_counter[type(obj)].add_repo_obj(obj)

    # Separate visitor methods in case we want to stop doing type introspection
    # pyre-fixme[4]: Attribute must be annotated.
    visit_repodata = _add_object
    # pyre-fixme[4]: Attribute must be annotated.
    visit_rpm = _add_object
    # pyre-fixme[4]: Attribute must be annotated.
    visit_repomd = _add_object

    def _get_classname_to_size(self) -> Dict[str, int]:
        return {
            t.__name__: c.total_size() for t, c in self._type_to_counter.items()
        }

    def get_report(self, msg: str) -> str:
        classname_to_size = self._get_classname_to_size()
        return f"""{msg} {sum(classname_to_size.values()):,} bytes, by type: {
            '; '.join(f'{n}: {s:,}' for n, s in classname_to_size.items())
        }"""
