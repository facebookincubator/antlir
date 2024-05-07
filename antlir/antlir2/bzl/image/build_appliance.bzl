# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        BuildApplianceInfo(
            dir = ctx.attrs.src,
        ),
        DefaultInfo(),
    ]

_build_appliance = rule(
    impl = _impl,
    attrs = {
        "src": attrs.source(),
    },
)

build_appliance = rule_with_default_target_platform(_build_appliance)
