# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl/image:mount_types.bzl", "mount_record")
load("//antlir/antlir2/features:feature_info.bzl", "feature_record")

FeatureInfo = provider(fields = [
    # Transitive set of feature records deserializable by Antlir tools
    "features",
])

FlavorInfo = provider(fields = [
    "dnf_info",  # FlavorDnfInfo provider for dnf-based distros
    "label",  # The buck label for this flavor
])

FlavorDnfInfo = provider(fields = [
    "default_excluded_rpms",  # The default set of rpms to exclude from all operations
    "default_extra_repo_set",  # The default set of extra dnf repos available to images of this flavor
    "default_repo_set",  # The default set of main dnf repos available to images of this flavor
    "default_versionlock",  # JSON file mapping package name -> EVRA
    "reflink_flavor",  # Key to identify rpm2extents output for a compatible version
])

# Eventually antlir2 hopes to support other storage formats, but for now only
# local subvolumes are supported
LayerContents = record(
    subvol_symlink = field(Artifact),  # symlink pointing to the built subvol
)

LayerInfo = provider(
    fields = {
        "contents": LayerContents,
        # Database managed by antlir2_facts
        "facts_db": Artifact,
        # List of all feature analyses
        "features": list[feature_record | typing.Any],
        # dep on the flavor this layer was built with
        "flavor": Dependency,
        # Label that originally created this layer
        "label": Label,
        # List of mount features
        "mounts": list[mount_record | typing.Any],
        # Dependency for the parent of the layer, if one exists
        "parent": Dependency | None,
        # LayerContents broken out by all the internal phases (for packages that
        # support incremental outputs)
        "phase_contents": list[(BuildPhase | typing.Any, LayerContents | typing.Any)],
    },
)

BuildApplianceInfo = provider(fields = {
    # For Build Appliance images, exact ownership and other fs metadata (aside
    # from executable bits, which RE will preserve) doesn't matter, so we can
    # directly use a plain directory as the build appliance.
    "dir": Artifact,
})
