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
import os
from enum import Enum, auto


def _normalize_path(path: str) -> str:
    # Normalize paths as image-absolute. This is crucial since we
    # will use `path` as a dictionary key.
    return os.path.normpath(
        # The `lstrip` is needed because `normpath does not
        # normalize away leading slashes: //b/c
        os.path.join("/", path.lstrip("/"))
    )


class _Predicate(Enum):
    IS_DIRECTORY = auto()
    IS_FILE = auto()


@dataclasses.dataclass(frozen=True)
class PathRequiresPredicate:
    path: str
    predicate: _Predicate

    def __init__(self, *, path: str, predicate: _Predicate) -> None:
        object.__setattr__(self, "path", _normalize_path(path))
        object.__setattr__(self, "predicate", predicate)


def require_directory(path: str):
    return PathRequiresPredicate(path=path, predicate=_Predicate.IS_DIRECTORY)


def require_file(path: str):
    return PathRequiresPredicate(path=path, predicate=_Predicate.IS_FILE)


@dataclasses.dataclass(frozen=True)
class ProvidesPathObject:
    path: str
    # In the future, we might add permissions, etc here.

    def __init__(self, *, path: str) -> None:
        object.__setattr__(self, "path", _normalize_path(path))

    def matches(
        self, path_to_reqs_provs, path_predicate: PathRequiresPredicate
    ) -> bool:
        assert (
            path_predicate.path == self.path
        ), "Tried to match {} against {}".format(path_predicate, self)
        assert self._matches_predicate(
            path_predicate.predicate
        ), "predicate {} not implemented by {}".format(path_predicate, self)
        return True

    def _matches_predicate(self, predicate):
        return False

    def with_new_path(self, new_path):
        return dataclasses.replace(self, path=_normalize_path(new_path))


class ProvidesDirectory(ProvidesPathObject):
    def _matches_predicate(self, predicate):
        return predicate == _Predicate.IS_DIRECTORY


class ProvidesFile(ProvidesPathObject):
    "Does not have to be a regular file, just any leaf in the FS tree"

    def _matches_predicate(self, predicate):
        return predicate == _Predicate.IS_FILE


class ProvidesDoNotAccess(ProvidesPathObject):
    # Deliberately matches no predicates -- this used to mark paths as "off
    # limits" to further writes.
    pass
