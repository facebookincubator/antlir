# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def ensure_single_output(
        dep: Dependency | Artifact | DefaultInfo | ProviderCollection,
        optional: bool = False) -> Artifact | None:
    if isinstance(dep, Artifact):
        return dep
    elif isinstance(dep, DefaultInfo):
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
