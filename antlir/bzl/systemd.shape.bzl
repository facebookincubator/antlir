# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":shape.bzl", "shape")

# Define shapes for systemd units. This is not intended to be an exhaustive
# list of every systemd unit setting from the start, but should be added to as
# more use cases generate units with these shapes.
unit_t = shape.shape(
    description = str,
    requires = shape.list(str, default = []),
    after = shape.list(str, default = []),
    before = shape.list(str, default = []),
)

fstype_t = shape.enum("btrfs", "9p")

mount_t = shape.shape(
    unit = unit_t,
    what = str,
    where = shape.path,
    # add more filesystem types here as required
    type = shape.field(fstype_t, optional = True),
    options = shape.list(str, default = []),
)
