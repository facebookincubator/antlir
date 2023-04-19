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
    # Transitive set of JSON files that contains the same contents as
    # `features_json`
    "json_files",
])

FlavorInfo = provider(fields = {
    "default_build_appliance": "The default build_appliance to use on images of this flavor",
    "default_rpm_repo_set": "The default set of rpm repos available to images of this flavor",
    "label": "The buck label for this flavor",
})

LayerInfo = provider(fields = {
    "depgraph": "JSON-serialized depgraph",
    "flavor_info": "The FlavorInfo this layer was built with",
    "label": "Label that originally created this layer",
    "mounts": "JSON artifact describing mounts",
    "parent": "LayerInfo from parent_layer, if any",
    "subvol_symlink": "symlink pointing to the built subvol",
})
