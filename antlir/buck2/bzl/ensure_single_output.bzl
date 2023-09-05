# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @starlark-rust: allow_string_literals_in_type_expr

def ensure_single_output(
        dep: Dependency | Artifact | DefaultInfo.type | "provider_collection",
        optional: bool = False) -> Artifact | None:
    if type(dep) == "artifact":
        return dep
    elif type(dep) == DefaultInfo.type:
        default_outputs = dep.default_outputs
    else:
        default_outputs = dep[DefaultInfo].default_outputs
    if not default_outputs:
        if optional:
            return None
        else:
            fail("'{}' does not produce any outputs".format(dep.label))
    if len(default_outputs) != 1:
        fail("'{}' did not have exactly one output: {}".format(dep, default_outputs))
    return default_outputs[0]
