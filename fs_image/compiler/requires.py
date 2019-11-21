#!/usr/bin/env python3
'''
See the docblock of `provides.py` for an explanation of the
Requires-Provides data model, and their interactions.

Future: we might want to add permissions constraints, tackle following
symlinks (or not following them), maybe hardlinks, etc.  This would
likely best be tackled via predicate composition with And/Or/Not support
with short-circuiting.  E.g. FollowsSymlinks(Pred) would expand to:

  Or(
    And(IsSymlink(Path), Pred(SymlinkTarget(Path))),
    And(Not(IsSymlink(Path)), Pred(SymlinkTarget(Path)),
  )

The predicates would then be wrapped into a PathObject.
'''
from collections import namedtuple

from .path_object import PathObject


class PathRequiresPredicate(metaclass=PathObject):
    fields = ['predicate']


IsDirectory = namedtuple('IsDirectory', [])
IsFile = namedtuple('IsFile', [])


def require_directory(path):
    return PathRequiresPredicate(path=path, predicate=IsDirectory())


def require_file(path):
    return PathRequiresPredicate(path=path, predicate=IsFile())
