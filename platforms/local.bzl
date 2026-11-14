# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Mark the host execution platform as capable of running local actions."""

def _impl(ctx: AnalysisContext) -> list[Provider]:
    constraints = dict(ctx.attrs.os[ConfigurationInfo].constraints)
    for dep in ctx.attrs.constraints:
        value = dep[ConstraintValueInfo]
        constraints[value.setting.label] = value
    return [DefaultInfo(), ConfigurationInfo(constraints = constraints, values = {})]

local_configuration = rule(
    impl = _impl,
    attrs = {
        "os": attrs.dep(providers = [ConfigurationInfo]),
        "constraints": attrs.list(attrs.dep(providers = [ConstraintValueInfo])),
    },
)
