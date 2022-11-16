# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Build info about a layer target
"""
LayerInfo = provider(fields = {
    "default_mountpoint": "Default location this will be mounted at in layers that use this as the `source` of a `layer_mount`",
    "features": "Features that get applied to build this image from the `parent_layer`",
    "flavor": "Flavor that configures how this image was built (build appliance, repo snapshot, etc)",
    "parent_layer": "Starting point for this layer before adding new features",
})
