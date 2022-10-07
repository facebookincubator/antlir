# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

LayerInfo = provider(fields = [
    # Buck target label
    "label",
    # Starting point for this layer
    "parent_layer",
    # Flavor that configures how this image is built (build appliance, repo snapshot etc)
    "flavor",
    # Features that get applied to build this image from the `parent_layer`
    "features",
    # Default location this will be mounted at in layers that use this as the
    # `source` of a `layer_mount`
    "default_mountpoint",
])
