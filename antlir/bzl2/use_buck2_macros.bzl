# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "is_buck2")

def use_buck2_macros():
    return is_buck2()  # and native.read_config("antlir", "use_buck2_macros")
