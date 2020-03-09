#!/usr/bin/env python3
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

  def matches_NameOfRequiresPredicate(self, path_to_reqs_provs, predicate):
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

The predicates would then be wrapped into a PathObject.
'''

from collections import namedtuple

from .path_object import PathObject


IsDirectory = namedtuple('IsDirectory', [])
IsFile = namedtuple('IsFile', [])


class PathRequiresPredicate(metaclass=PathObject):
    fields = ['predicate']


def require_directory(path):
    return PathRequiresPredicate(path=path, predicate=IsDirectory())


def require_file(path):
    return PathRequiresPredicate(path=path, predicate=IsFile())


class ProvidesPathObject:
    __slots__ = ()
    fields = []  # In the future, we might add permissions, etc here.

    def matches(self, path_to_reqs_provs, path_predicate):
        assert path_predicate.path == self.path, (
            'Tried to match {} against {}'.format(path_predicate, self)
        )
        fn = getattr(
            self, 'matches_' + type(path_predicate.predicate).__name__, None
        )
        assert fn is not None, (
            'predicate {} not implemented by {}'.format(path_predicate, self)
        )
        return fn(path_to_reqs_provs, path_predicate.predicate)


class ProvidesDirectory(ProvidesPathObject, metaclass=PathObject):
    def matches_IsDirectory(self, _path_to_reqs_provs, predicate):
        return True


class ProvidesFile(ProvidesPathObject, metaclass=PathObject):
    'Does not have to be a regular file, just any leaf in the FS tree'
    def matches_IsFile(self, _path_to_reqs_provs, predicate):
        return True


class ProvidesDoNotAccess(ProvidesPathObject, metaclass=PathObject):
    # Deliberately matches no predicates -- this used to mark paths as "off
    # limits" to further writes.
    pass
