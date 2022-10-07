# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")

def flatten(lst):
    flat = []
    for item in lst:
        if types.is_list(item) or types.is_tuple(item):
            # @lint-ignore BUCKRESTRICTEDSYNTAX
            flat.extend(flatten(item))
        else:
            flat.append(item)

    return flat
