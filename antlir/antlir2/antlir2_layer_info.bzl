# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

LayerInfo = provider(fields = {
    "depgraph": "JSON-serialized depgraph",
    "flavor_info": "The FlavorInfo this layer was built with",
    "label": "Label that originally created this layer",
    "mounts": "JSON artifact describing mounts",
    "parent": "LayerInfo from parent_layer, if any",
    "subvol_symlink": "symlink pointing to the built subvol",
})
