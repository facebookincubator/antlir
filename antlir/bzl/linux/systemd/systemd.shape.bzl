# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

# Minimal definitions of some systemd unit types. This is not exhaustive and
# should be added to for any new fields that need to be supported.

unit_t = shape.shape(
    description = str,
    after = shape.field(shape.list(str), default = []),
    requires = shape.field(shape.list(str), default = []),
)

service_t = shape.shape(
    unit = unit_t,
    type = shape.field(str, default = "oneshot"),
    slice = shape.field(str, optional = True),
    environment_file = shape.field(str, optional = True),
    exec_start = shape.field(shape.list(str), optional = True),
    standard_output = shape.field(str, optional = True),
)

timer_t = shape.shape(
    unit = unit_t,
    on_calendar = shape.field(str, optional = True),
    accuracy = shape.field(str, optional = True),
    on_boot = shape.field(str, optional = True),
    persistent = shape.field(bool, default = False),
    randomized_delay = shape.field(str, optional = True),
    unit_name = shape.field(str, optional = True),
)
