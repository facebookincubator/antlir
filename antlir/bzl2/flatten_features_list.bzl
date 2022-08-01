# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:structs.bzl", "structs")

def flatten_features_list(lst):
    """
    Recursively extracts feature targets from lists within `lst`.
    """
    flattened_list = []
    for item in lst:
        if not item:
            continue
        if types.is_string(item) or structs.is_struct(item):
            flattened_list.append(item)
        else:
            flattened_list.extend(flatten_features_list(item))
    return flattened_list
