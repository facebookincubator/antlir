#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
If two distinct copies of an RPM with the same NEVRA are committed to a repo
(e.g. due to signing key changes), this will trip "mutable_rpm_error" and
prevent either RPM from being accessed.

We consider mutable RPMs to be a bug, but if the bugs do happen, we need
to be able to delete any version we consider non-canonical, and resume
access.

The mapping in this file provides a remediation for this case.

Importantly, `repo_downloader.py` will FAIL LOUDLY if an RPM is marked
remediated while it still exists in the repos.  This is the only correct
behavior because of how its mutable RPM detection is designed.

NB: It is preferred to hardcode the hash algorithm, rather than to import
`CANONICAL_HASH`, because in this case, the checksum cannot accidentally and
silently change if `CANONICAL_HASH` is redefined.

Design rationale: These "remediated" hashes are not in the database, because:
  - We don't expect to have many of these remediations, and this is simpler.
  - Reproducibility of builds requires that marking a mutable RPM instance
    as "deleted" should have no effect on older builds, when the RPM was not
    yet remediated -- those should still produce "mutable_error".  We could
    store a timestamp in the DB and only remediate hashes older than the
    current source tree, but this is just a more complicated way of pinning
    the remediated RPMs to the source tree.
"""
deleted_mutable_rpms = {
    # ("universe", Rpm.nevra()): {Checksum(...), Checksum(...)},
}

try:
    from antlir.rpm.downloader.facebook.deleted_mutable_rpms import (
        deleted_mutable_rpms as _fb_deleted_mutable_rpms,
    )

    deleted_mutable_rpms.update(_fb_deleted_mutable_rpms)
except ImportError:  # pragma: no cover
    pass


from antlir.rpm.repo_objects import CANONICAL_HASH


for _checksums in deleted_mutable_rpms.values():
    for _checksum in _checksums:
        assert _checksum.algorithm == CANONICAL_HASH
