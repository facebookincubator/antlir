# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

gpt_partition_t = shape.shape(
    package = target_t,
    is_esp = bool,
    name = shape.field(str, optional = True),
)

gpt_t = shape.shape(
    name = str,
    disk_guid = shape.field(str, optional = True),
    table = shape.list(gpt_partition_t),
)
