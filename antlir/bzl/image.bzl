# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load(":constants.bzl", "new_nevra")
load(":image_layer.bzl", "image_layer")

image = struct(
    layer = image_layer,
    opts = struct,
    rpm = struct(nevra = new_nevra),
)
