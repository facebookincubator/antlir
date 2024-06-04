# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_overlayfs:overlayfs.bzl", "OverlayFs")

FeatureInfo = provider(fields = [
    # Transitive set of feature records deserializable by Antlir tools
    "features",
])

FlavorInfo = provider(fields = [
    "default_build_appliance",  # The default build_appliance to use on images of this flavor
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

# A layer is backed by either a local btrfs subvol or antlir2_overlayfs.
# Eventually all layers will be backed by overlayfs (TODO(T156455885)), but
# while RE builds are being worked on, most are still a local subvol.
LayerContents = record(
    subvol_symlink = field(Artifact | None, default = None),  # symlink pointing to the built subvol
    overlayfs = field(OverlayFs | None, default = None),  # antlir2_overlayfs record
)

LayerInfo = provider(fields = [
    "build_appliance",  # dep on the build appliance that was use to build this (if any)
    "facts_db",  # Database managed by antlir2_facts
    "features",  # List of all feature analyses
    "flavor",  # dep on the flavor this layer was built with
    "flavor_info",  # The FlavorInfo this layer was built with
    "label",  # Label that originally created this layer
    "mounts",  # List of mount features
    "parent",  # Dependency for the parent of the layer, if one exists
    "contents",  # LayerContents record
    # Symlink to local subvolume in antlir2-out/. Kept around for a
    # transitionary period while the whole antlir ecosystem grows to support
    # antlir2_overlayfs
    "subvol_symlink",
])

BuildApplianceInfo = provider(fields = {
    # For Build Appliance images, exact ownership and other fs metadata (aside
    # from executable bits, which RE will preserve) doesn't matter, so we can
    # directly use a plain directory as the build appliance.
    "dir": Artifact,
})
