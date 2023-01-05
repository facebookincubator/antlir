# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def ensure_single_output(dep: "dependency", optional: bool.type = True) -> ["artifact", None]:
    default_outputs = dep[DefaultInfo].default_outputs
    if optional and len(default_outputs) == 0:
        return None
    if len(default_outputs) > 1:
        fail("'{}' did not have exactly one output: {}".format(dep, default_outputs))
    return default_outputs[0]
