# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:snapshot_install_dir.bzl", "RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR", "snapshot_install_dir")
load(":layer_info.bzl", "LayerInfo")

FlavorInfo = provider(fields = [
    "build_appliance",
    "name",
    "rpm_installer",
    "rpm_repo_snapshot",
    "rpm_version_set_overrides",
    "version_set_path",
])

def _impl(ctx: "context") -> ["provider"]:
    # TODO(T133597736) remove rpm snapshots from the image and use a real target
    rpm_repo_snapshot = None
    if ctx.attrs.rpm_repo_snapshot:
        rpm_repo_snapshot = snapshot_install_dir(str(ctx.attrs.rpm_repo_snapshot.label.raw_target()))
    else:
        rpm_repo_snapshot = paths.join(RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR, ctx.attrs.rpm_installer)
    return [
        FlavorInfo(
            build_appliance = ctx.attrs.build_appliance,
            name = ctx.label.name,
            rpm_installer = ctx.attrs.rpm_installer,
            rpm_repo_snapshot = rpm_repo_snapshot,
            rpm_version_set_overrides = ctx.attrs.rpm_version_set_overrides,
            version_set_path = ctx.attrs.version_set_path,
        ),
        DefaultInfo(default_outputs = []),
    ]

RpmRepoSnapshotInfo = provider(fields = [])

flavor = rule(
    impl = _impl,
    attrs = {
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo])),
        "rpm_installer": attrs.option(attrs.enum(["dnf", "yum"])),
        # TODO(T133597736): proper provider for this
        "rpm_repo_snapshot": attrs.option(attrs.dep()),
        "rpm_version_set_overrides": attrs.list(attrs.tuple(
            attrs.string(),  # name
            attrs.string(),  # epoch
            attrs.string(),  # version
            attrs.string(),  # release
            attrs.string(),  # arch
        ), default = []),
        "version_set_path": attrs.option(attrs.string()),
    },
)
