# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")

TTY_NAME = arch_select(aarch64 = "ttyAMA0", x86_64 = "ttyS0")
