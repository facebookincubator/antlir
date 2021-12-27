# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/linux/config/glibc:nsswitch.bzl", "nsswitch")
load("//antlir/bzl/linux/config/network:resolv.bzl", "resolv")

config = struct(
    network = struct(
        resolv = resolv,
    ),
    glibc = struct(
        nsswitch = nsswitch,
    ),
)
