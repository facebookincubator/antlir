# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# All LLVM versions supported by antlir across OS flavors. Not every OS has
# every version - see `_LLVM_VERSIONS_BY_OS` below.
ANTLIR_LLVM_VERSIONS = ["19", "20", "21"]

# Default LLVM version — standard across all OS flavors that ship it.
DEFAULT_LLVM_VERSION = "20"

# OSes whose repos cannot provide every version in ANTLIR_LLVM_VERSIONS.
# CentOS 9 Stream no longer ships clang/lld 20, and debian-trixie packages
# llvm 19 only. Other OSes ship the default set below (llvm 19 is only
# packaged by debian, so it is excluded from the default set).
_LLVM_VERSIONS_BY_OS = {
    "centos9": ["21"],
    "debian-trixie": ["19"],
}

def llvm_versions_for_os(os: str) -> list[str]:
    return _LLVM_VERSIONS_BY_OS.get(os, [v for v in ANTLIR_LLVM_VERSIONS if v != "19"])

def default_llvm_version_for_os(os: str) -> str:
    versions = llvm_versions_for_os(os)
    return DEFAULT_LLVM_VERSION if DEFAULT_LLVM_VERSION in versions else versions[-1]
