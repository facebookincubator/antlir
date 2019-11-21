#!/usr/bin/env python3
'''
Repos in `yum.conf` can (and do) share Repodata and Rpm objects, so the best
estimate of their total space usage requires counting each object only once.

`RepoDownloader` feeds the requisite information to this visitor. This
implements the `RepoObjectVisitor` interface, featuring these methods:
    def visit_repomd(self, repomd: RepoMetadata) -> None:
    def visit_repodata(self, repodata: Repodata) -> None:
    def visit_rpm(self, rpm: Rpm) -> None:
In the future, it can be officially declared, if useful.
'''
from collections import defaultdict
from typing import Dict, NamedTuple, Set


class Synonyms(NamedTuple):
    size: int
    # Different checksums can refer to the same file (see `_ObjectCounter.add`)
    synonyms: Set['Checksum']
    # This is the canonical representative of all the synonyms -- but not
    # necessarily a CANONICAL_HASH.
    primary: 'Checksum'


class _ObjectCounter:
    def __init__(self):
        # An alternative to keying everything on checksum would be to use
        # keys like `checksum` for `Repodata` and `filename` for `Rpm`.
        # Uniformly using checksums gracefully handles `MutableRpmError`,
        # and keeps this code generic.
        self._checksum_to_synonyms = {}

    def _get_synonyms(self, checksum: 'Checksum', size: int) -> Synonyms:
        synonyms = self._checksum_to_synonyms.get(checksum)
        return synonyms if synonyms is not None \
            else Synonyms(size=size, primary=checksum, synonyms=set())

    def add(self, obj):
        # IMPORTANT: We do make no updates until all sanity-checks have been
        # done -- a user error shouldn't corrupt our prior state.
        best_synonyms = self._get_synonyms(obj.best_checksum(), obj.size)
        assert best_synonyms.size == obj.size, \
            f'{obj} best checksum has prior size {best_synonyms.size}'

        # If we have two checksums, merge their synonym sets.
        #
        # RPMs may be hashed with different algorithms in different repos.
        # To avoid double-counting these, `best_checksum` provides the
        # canonical checksum, if available.
        #
        # When `--rpm-shard` is in use, we won't have the canonical checksum
        # for RPMs outside of our shard (or within the shard, for RPMs that
        # failed to download).  This means we may double-count those RPMs
        # that occur in multiple repos, with the repos using different hash
        # algorithms.
        if obj.best_checksum() != obj.checksum:
            # Again, perform all checks before mutating state.
            other_synonyms = self._get_synonyms(obj.checksum, obj.size)
            assert other_synonyms.size == obj.size, \
                f'{obj} other checksum has prior size {other_synonyms.size}'
            for synonym in other_synonyms.synonyms:
                # No equivalent check for .primary because this might be a
                # brand-new object, and if it isn't, this check is trivial.
                assert self._checksum_to_synonyms[synonym] is other_synonyms
            assert other_synonyms.size == best_synonyms.size  # redundant
            # All checks passed, so we can merge `other_synonyms` into
            # `best_synonyms`.
            best_synonyms.synonyms.update(other_synonyms.synonyms)
            for synonym in other_synonyms.synonyms:
                self._checksum_to_synonyms[synonym] = best_synonyms
            best_synonyms.synonyms.add(other_synonyms.primary)
            self._checksum_to_synonyms[other_synonyms.primary] = best_synonyms

        # All checks passed, it is now safe to mutate state
        self._checksum_to_synonyms[obj.best_checksum()] = best_synonyms

    def total_size(self):
        return sum(
            s.size for c, s in self._checksum_to_synonyms.items()
                if s.primary == c
        )


class RepoSizer:
    def __init__(self):
        # Count each type of objects separately
        self._type_to_counter = defaultdict(_ObjectCounter)

    def _add_object(self, obj):
        self._type_to_counter[type(obj)].add(obj)

    # Separate visitor methods in case we want to stop doing type introspection
    visit_repodata = _add_object
    visit_rpm = _add_object
    visit_repomd = _add_object

    def _get_classname_to_size(self) -> Dict[str, int]:
        return {
            t.__name__: c.total_size()
                for t, c in self._type_to_counter.items()
        }

    def get_report(self, msg: str) -> str:
        classname_to_size = self._get_classname_to_size()
        return f'''{msg} {sum(classname_to_size.values()):,} bytes, by type: {
            '; '.join(f'{n}: {s:,}' for n, s in classname_to_size.items())
        }'''
