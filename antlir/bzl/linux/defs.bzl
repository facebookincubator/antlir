# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/linux/config:config.bzl", "config")
load(":busybox.bzl", "busybox")
load(":filesystem.bzl", "filesystem")
load(":time.bzl", "time")

# This exposed struct provides a clean API for clients
# to use.
linux = struct(
    busybox = busybox,
    config = config,
    filesystem = filesystem,
    time = time,
)
