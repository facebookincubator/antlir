# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

FeatureInfo = provider(fields = [
    # FeatureDeps transitive set
    # Transitive set of the artifacts that must be materialized on disk for the
    # compiler to be able to build this feature
    "required_artifacts",
    # Runnable binaries required to build this feature.
    "required_run_infos",
    # Transitive set of the image layers that are required to be already-build
    # before building this feature
    "required_layers",
    # Transitive set of feature records deserializable by Antlir tools
    "features",
])

FlavorInfo = provider(fields = {
    "default_build_appliance": "The default build_appliance to use on images of this flavor",
    "dnf_info": "FlavorDnfInfo provider for dnf-based distros",
    "label": "The buck label for this flavor",
})

FlavorDnfInfo = provider(fields = {
    "default_excluded_rpms": "The default set of rpms to exclude from all operations",
    "default_repo_set": "The default set of dnf repos available to images of this flavor",
    "default_versionlock": "JSON file mapping package name -> EVRA",
})

LayerInfo = provider(fields = {
    "build_appliance": "dep on the build appliance that was use to build this (if any)",
    "depgraph": "JSON-serialized depgraph",
    "flavor": "dep on the flavor this layer was built with",
    "flavor_info": "The FlavorInfo this layer was built with",
    "label": "Label that originally created this layer",
    "mounts": "List of mount features",
    "parent": "LayerInfo from parent_layer, if any",
    "subvol_symlink": "symlink pointing to the built subvol",
})
