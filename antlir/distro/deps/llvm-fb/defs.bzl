# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

libs = [
    "clang_rt.asan",
    "clang_rt.asan_cxx",
    "clang_rt.dfsan",
    "clang_rt.hwasan",
    "clang_rt.hwasan_cxx",
    "clang_rt.msan",
    "clang_rt.msan_cxx",
    "clang_rt.tsan",
    "clang_rt.tsan_cxx",
    "clang_rt.ubsan_standalone",
    "clang_rt.ubsan_standalone_cxx",
]

def clang_rt_library(*, name: str):
    native.prebuilt_cxx_library(
        name = name,
        # TODO: does this need headers too?
        shared_lib = ":libs[{}]".format(name),
        preferred_linkage = "shared",
        visibility = ["PUBLIC"],
    )
