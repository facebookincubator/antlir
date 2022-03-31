# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/linux/boot/grub2:defs.bzl", "grub2_boot")
load("//antlir/bzl/linux/boot/systemd:defs.bzl", "systemd_boot")

boot = struct(
    grub2 = grub2_boot,
    systemd = systemd_boot,
)
