# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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

LayerInfo = provider(fields = [
    "facts_db",  # Database managed by antlir2_facts
    "features",  # List of all feature analyses
    "flavor",  # dep on the flavor this layer was built with
    "flavor_info",  # The FlavorInfo this layer was built with
    "label",  # Label that originally created this layer
    "mounts",  # List of mount features
    "parent",  # Dependency for the parent of the layer, if one exists
    "contents",  # LayerContents record
    "phase_contents",  # LayerContents broken out by all the internal phases
])

BuildApplianceInfo = provider(fields = {
    # For Build Appliance images, exact ownership and other fs metadata (aside
    # from executable bits, which RE will preserve) doesn't matter, so we can
    # directly use a plain directory as the build appliance.
    "dir": Artifact,
})
