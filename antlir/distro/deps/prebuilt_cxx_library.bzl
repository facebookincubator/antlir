# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def prebuilt_cxx_library(*, labels = None, extract_soname = None, **kwargs):
    native.prebuilt_cxx_library(
        labels = (labels or []) + ["antlir-distro-dep"],
        extract_soname = True if extract_soname == None else extract_soname,
        **kwargs
    )
