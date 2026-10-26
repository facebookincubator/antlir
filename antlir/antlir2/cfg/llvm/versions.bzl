# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# All LLVM versions supported by antlir across OS flavors.
ANTLIR_LLVM_VERSIONS = ["20", "21"]

# Default LLVM version — standard across all OS flavors.
DEFAULT_LLVM_VERSION = "20"

# OSes whose repos cannot provide every version in ANTLIR_LLVM_VERSIONS.
# CentOS 9 Stream no longer ships clang/lld 20.
_LLVM_VERSIONS_BY_OS = {
    "centos9": ["21"],
}

def llvm_versions_for_os(os: str) -> list[str]:
    return _LLVM_VERSIONS_BY_OS.get(os, ANTLIR_LLVM_VERSIONS)

def default_llvm_version_for_os(os: str) -> str:
    versions = llvm_versions_for_os(os)
    return DEFAULT_LLVM_VERSION if DEFAULT_LLVM_VERSION in versions else versions[-1]
