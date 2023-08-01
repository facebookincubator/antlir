#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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

from antlir.fs_utils import Path


@dataclasses.dataclass(frozen=True)
class Requirement:
    pass


def _normalize_path(path: Path) -> Path:
    # Normalize paths as image-absolute. This is crucial since we
    # will use `path` as a dictionary key.
    return Path(b"/" / path.strip_leading_slashes()).normpath()


@dataclasses.dataclass(frozen=True)
# pyre-fixme[13]: Attribute `path` is never initialized.
class RequirePath(Requirement):
    path: Path

    def __init__(self, path: Path) -> None:
        super().__init__()
        object.__setattr__(self, "path", _normalize_path(path))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class RequireDirectory(RequirePath):
    pass


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class RequireFile(RequirePath):
    pass


class _RequireDoNotAccess(RequirePath):
    # Only ProvidesDoNotAccess should instantiate this type of RequirePath and
    # it is meant to fail compilation if a RequireDirectory or RequireFile is
    # requested at this path.
    pass


@dataclasses.dataclass(frozen=True)
# pyre-fixme[13]: Attribute `target` is never initialized.
class RequireSymlink(RequirePath):
    target: Path

    def __init__(self, path: Path, target: Path) -> None:
        super().__init__(path=path)
        object.__setattr__(self, "target", target)


@dataclasses.dataclass(frozen=True)
# pyre-fixme[13]: Attribute `name` is never initialized.
class RequireGroup(Requirement):
    name: str

    def __init__(self, name: str) -> None:
        super().__init__()
        object.__setattr__(self, "name", name)


@dataclasses.dataclass(frozen=True)
# pyre-fixme[13]: Attribute `name` is never initialized.
class RequireUser(Requirement):
    name: str

    def __init__(self, name: str) -> None:
        super().__init__()
        object.__setattr__(self, "name", name)


@dataclasses.dataclass(frozen=True)
# pyre-fixme[13]: Attribute `tag` is never initialized.
class RequireKey(Requirement):
    key: str

    def __init__(self, key: str) -> None:
        super().__init__()
        object.__setattr__(self, "key", key)


@dataclasses.dataclass(frozen=True)
class Provider:
    req: Requirement

    def provides(self, req: Requirement) -> bool:
        return self.req == req


@dataclasses.dataclass(frozen=True)
class ProvidesPath(Provider):
    req: RequirePath

    def path(self) -> Path:
        return self.req.path

    def with_new_path(self, new_path: Path) -> "ProvidesPath":
        # pyre-fixme[6]: Expected `RequirePath` for 1st param but got `Path`.
        return self.__class__(new_path)


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesDirectory(ProvidesPath):
    def __init__(self, path: Path) -> None:
        super().__init__(req=RequireDirectory(path=path))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesFile(ProvidesPath):
    "Does not have to be a regular file, just any leaf in the FS tree"

    def __init__(self, path: Path) -> None:
        super().__init__(req=RequireFile(path=path))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesSymlink(ProvidesPath):
    def __init__(self, path: Path, target: Path) -> None:
        super().__init__(req=RequireSymlink(path, target))

    def with_new_path(self, new_path: Path) -> "ProvidesSymlink":
        # pyre-fixme[16]: `RequirePath` has no attribute `target`.
        return self.__class__(new_path, self.req.target)


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesDoNotAccess(ProvidesPath):
    def __init__(self, path: Path) -> None:
        super().__init__(req=_RequireDoNotAccess(path=path))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesGroup(Provider):
    def __init__(self, groupname: str) -> None:
        super().__init__(req=RequireGroup(groupname))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesUser(Provider):
    def __init__(self, username: str) -> None:
        super().__init__(req=RequireUser(username))


@dataclasses.dataclass(init=False, repr=False, eq=False, frozen=True)
class ProvidesKey(Provider):
    def __init__(self, key) -> None:
        super().__init__(req=RequireKey(key))
