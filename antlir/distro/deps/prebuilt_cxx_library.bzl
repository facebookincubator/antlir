# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def prebuilt_cxx_library(*, labels = None, **kwargs):
    native.prebuilt_cxx_library(
        labels = (labels or []) + ["antlir-distro-dep"],
        **kwargs
    )
