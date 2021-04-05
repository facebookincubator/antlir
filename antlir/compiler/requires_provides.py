#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
Images are composed of a bunch of Items. These are declared by the user
in an order-independent fashion, but they have to be installed in a specific
order. For example, we can only copy a file into a directory after the
directory already exists.

The main jobs of the image compiler are:
 - to validate that the specified Items will work well together, and
 - to install them in the appropriate order.

To do these jobs, each Item Provides certain filesystem features --
described in this file -- and also Requires certain predicates about
filesystem features -- described in `requires.py`.

Requires and Provides must interact in some way -- either
 (1) Provides objects need to know when they satisfy each requirements, or
 (2) Requires objects must know all the Provides that satisfy them.

The first arrangement seemed more maintainable, so each Provides object has
to define its relationship with every Requires predicate, thus:

  def matches(self, path_to_reqs_provs, predicate):
      """
      `path_to_reqs_provs` is the map constructed by `ValidatedReqsProvs`.
      This is a breadcrumb for the future -- having the full set of
      "provides" objects will let us resolve symlinks.
      """
      return True or False

Future: we might want to add permissions constraints, tackle following
symlinks (or not following them), maybe hardlinks, etc.  This would
likely best be tackled via predicate composition with And/Or/Not support
with short-circuiting.  E.g. FollowsSymlinks(Pred) would expand to:

  Or(
    And(IsSymlink(Path), Pred(SymlinkTarget(Path))),
    And(Not(IsSymlink(Path)), Pred(SymlinkTarget(Path)),
  )
'''
import dataclasses
from enum import Enum, auto
from typing import Hashable

from antlir.fs_utils import Path


class RequirementKind(Enum):
    PATH = auto()
    GROUP = auto()
    USER = auto()


@dataclasses.dataclass(frozen=True)
class Requirement:
    kind: RequirementKind

    def key(self) -> Hashable:
        raise NotImplementedError("Requirements must implement key")


class _Predicate(Enum):
    IS_DIRECTORY = auto()
    IS_FILE = auto()


def _normalize_path(path: Path) -> Path:
    # Normalize paths as image-absolute. This is crucial since we
    # will use `path` as a dictionary key.
    return Path(b"/" / path.strip_leading_slashes()).normpath()


@dataclasses.dataclass(frozen=True)
class PathRequiresPredicate(Requirement):
    path: Path
    predicate: _Predicate

    def __init__(self, path: Path, predicate: _Predicate) -> None:
        super().__init__(kind=RequirementKind.PATH)
        object.__setattr__(self, "path", _normalize_path(path))
        object.__setattr__(self, "predicate", predicate)

    def key(self) -> Hashable:
        return self.path


def require_directory(path: Path):
    return PathRequiresPredicate(path=path, predicate=_Predicate.IS_DIRECTORY)


def require_file(path: Path):
    return PathRequiresPredicate(path=path, predicate=_Predicate.IS_FILE)


@dataclasses.dataclass(frozen=True)
class RequireGroup(Requirement):
    name: str

    def __init__(self, name: str) -> None:
        super().__init__(kind=RequirementKind.GROUP)
        object.__setattr__(self, "name", name)

    def key(self) -> Hashable:
        return self.__hash__()


@dataclasses.dataclass(frozen=True)
class RequireUser(Requirement):
    name: str

    def __init__(self, name: str) -> None:
        super().__init__(kind=RequirementKind.USER)
        object.__setattr__(self, "name", name)

    def key(self) -> Hashable:
        return self.__hash__()


@dataclasses.dataclass(frozen=True)
class Provider:
    req: Requirement

    def provides(self, req: Requirement) -> bool:
        return self.req == req


@dataclasses.dataclass(frozen=True)
class ProvidesPath(Provider):
    req: PathRequiresPredicate

    def path(self) -> Path:
        return self.req.path

    def with_new_path(self, new_path: Path):
        return self.__class__(new_path)


class ProvidesDirectory(ProvidesPath):
    def __init__(self, path: Path):
        super().__init__(req=require_directory(path))


class ProvidesFile(ProvidesPath):
    "Does not have to be a regular file, just any leaf in the FS tree"

    def __init__(self, path: Path):
        super().__init__(req=require_file(path))


class ProvidesDoNotAccess(ProvidesPath):
    # Deliberately matches no predicates -- this used to mark paths as "off
    # limits" to further writes.

    def __init__(self, path: Path):
        super().__init__(req=PathRequiresPredicate(path, None))


class ProvidesGroup(Provider):
    def __init__(self, groupname: str):
        super().__init__(req=RequireGroup(groupname))


class ProvidesUser(Provider):
    def __init__(self, username: str):
        super().__init__(req=RequireUser(username))
