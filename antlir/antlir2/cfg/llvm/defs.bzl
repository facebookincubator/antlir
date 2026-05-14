# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/cfg/llvm/facebook:defs.bzl", _llvm_refs = "fb_llvm_refs")
load(":versions.bzl", "ANTLIR_LLVM_VERSIONS")

_llvm_refs = {"llvm_setting": "antlir//antlir/antlir2/cfg/llvm:llvm-version"} | {"llvm." + v: "antlir//antlir/antlir2/cfg/llvm:llvm-version[" + v + "]" for v in ANTLIR_LLVM_VERSIONS} # @oss-enable

def _transition(*, constraints, refs: struct, attrs: struct, overwrite: bool = False):
    setting = refs.llvm_setting[ConstraintSettingInfo]
    if attrs.default_llvm_version and ((setting.label not in constraints) or overwrite):
        constraint = getattr(refs, "llvm." + attrs.default_llvm_version)[ConstraintValueInfo]
        constraints[setting.label] = constraint
    return constraints

llvm_cfg = struct(
    transition = _transition,
    refs = _llvm_refs,
    attrs = {
        "default_llvm_version": attrs.option(attrs.enum(ANTLIR_LLVM_VERSIONS), default = None),
    },
)
