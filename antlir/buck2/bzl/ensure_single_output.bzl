# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def ensure_single_output(dep: "dependency") -> "artifact":
    default_outputs = dep[DefaultInfo].default_outputs
    if len(default_outputs) != 1:
        fail("'{}' did not have exactly one output".format(dep))
    return default_outputs[0]
