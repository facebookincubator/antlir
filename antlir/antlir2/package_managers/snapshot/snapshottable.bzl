# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

SnapshottableInfo = provider(
    fields = {
        # The architectures this repo supports.
        #  - For yum repos these are values like "aarch64"/"x86_64" — the
        #    BXL analyzes the target once per arch to materialize per-arch
        #    metadata trees and package indexes, then collapses the results
        #    into a single generated `repo()` call with a `select()` on
        #    `repomd_checksums`.
        #  - For deb suites the rule produces an all-arches metadata tree
        #    in one shot; the snapshot binary preserves this list as the
        #    `architectures` attr on the regenerated `suite()` call so the
        #    snapshotted target re-snapshots identically.
        "arches": list[str],
        # A directory artifact that should contain all the metadata artifacts that
        # should be snapshotted into persistent storage. This is considered cheap to
        # produce and store, as opposed to package blobs (which are belligerent and
        # numerous https://www.youtube.com/watch?v=HvVJQMnB-N8), so the target that
        # contains this provider should just produce the entire tree of metadata
        # artifacts every time.
        "metadata_tree": Artifact,
        # Names of packages within the repo that should be exposed as
        # individually-buildable subtargets. Preserved verbatim into the
        # generated BUCK file so consumers of the snapshotted repo keep
        # working.
        "package_subtargets": list[str],
        # The base url to which package "filename"s are relative
        "packages_baseurl": str,
        # A list of JSON artifacts that describe packages that need to be
        # snapshotted to persistent storage to make this repo reproducible.
        # These are expensive to snapshot, so the output here is simply an index
        # where each file is a list of package structs of the form
        # `{"checksums": ..., "upstream_url": ...}
        # The snapshotter can then dedupe based on checksum and only download the
        # blobs that have not already been preserved
        "packages_indexes": list[Artifact],
        # Optional alternate path for the generated BUCK file, relative to
        # `fbcode/bot_generated/antlir/snapshot/` or absolute under that root.
        # For example, `kernel/6.13/BUCK` will be materialized as
        # `fbcode/bot_generated/antlir/snapshot/kernel/6.13/BUCK`. Multiple repos
        # can share the same path to be co-located in a single BUCK file.
        "snapshot_buck_file": [str, None],
        # Storage backend config used when snapshotting this repo. Required
        # to actually run a snapshot — analysis still succeeds without it,
        # but the snapshot binary will refuse to operate on the target.
        #
        # Shape matches the snapshot binary's StorageConfig serde format —
        # for Manifold: `{"type": "manifold", "bucket": "...", "api_key":
        # "..."}`. `tree_prefix` is derived per-target by the binary and
        # must NOT be set here.
        "snapshot_storage": [dict[str, str], None],
    },
)
