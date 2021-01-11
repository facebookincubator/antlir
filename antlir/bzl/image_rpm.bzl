# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def nevra(*, name, epoch, version, release, arch):
    return struct(_private_envra = [epoch, name, version, release, arch])

image_rpm = struct(
    nevra = nevra,
)
