# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "buck_filegroup", "buck_sh_test")
load("//antlir/bzl:query.bzl", "query")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep", "targets_and_outputs_arg_list")

def starlark_unittest(
        name,
        srcs,
        deps):
    buck_filegroup(
        name = name + "--srcs",
        srcs = srcs,
    )
    buck_sh_test(
        name = name,
        args = ["$(location :{}--srcs)".format(name)] + targets_and_outputs_arg_list(name, query.set(deps)) + ["--"],
        test = antlir_dep("bzl/starlark_unittest:starlark-unittest"),
        type = "rust",
    )
