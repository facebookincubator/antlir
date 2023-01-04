# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# TODO(vmagro): this is still pretty focused on RPMs and our current RPM
# snapshot structure, but this will be more reasonably refactored when this is
# the main source of truth

DistroInfo = provider(fields = {
    "package_manager": "PackageManagerInfo provider",
    "snapshot": "SnapshotInfo provider",
})

PackageManagerInfo = provider(fields = {
    "type": "None, or 'dnf'",
})

SnapshotInfo = provider(fields = {
    "storage_id": "snapshot.storage_id source file",
    "version_set_target_prefix": "target base path to rpm version set targets",
})

def _rpm_distro_impl(ctx: "context") -> ["provider"]:
    return [
        DistroInfo(
            package_manager = PackageManagerInfo(
                type = "dnf",
            ),
            snapshot = SnapshotInfo(
                storage_id = ctx.attrs.snapshot_storage_id,
                version_set_target_prefix = ctx.attrs.version_set_target_prefix,
            ),
        ),
        DefaultInfo(),
    ]

rpm_distro = rule(
    impl = _rpm_distro_impl,
    attrs = {
        "snapshot_storage_id": attrs.source(),
        "version_set_target_prefix": attrs.string(),
    },
)
