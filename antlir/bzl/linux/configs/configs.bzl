# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/linux/configs/glibc:nsswitch.bzl", "nsswitch")
load("//antlir/bzl/linux/configs/network:resolv.bzl", "resolv")

configs = struct(
    network = struct(
        resolv = resolv,
    ),
    glibc = struct(
        nsswitch = nsswitch,
    ),
)
