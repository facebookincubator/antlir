# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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
    "default_repo_set",  # The default set of dnf repos available to images of this flavor
    "default_versionlock",  # JSON file mapping package name -> EVRA
    "reflink_flavor",  # Key to identify rpm2extents output for a compatible version
])

LayerInfo = provider(fields = [
    "build_appliance",  # dep on the build appliance that was use to build this (if any)
    "depgraph",  # JSON-serialized depgraph
    "features",  # List of all feature analyses
    "flavor",  # dep on the flavor this layer was built with
    "flavor_info",  # The FlavorInfo this layer was built with
    "label",  # Label that originally created this layer
    "mounts",  # List of mount features
    "parent",  # Dependency for the parent of the layer, if one exists
    "subvol_symlink",  # symlink pointing to the built subvol
])
