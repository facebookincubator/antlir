# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo")

def _build_appliance_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        BuildApplianceInfo(
            dir = ctx.attrs.archive,
        ),
    ]

_build_appliance_rule = rule(
    impl = _build_appliance_impl,
    attrs = {
        "archive": attrs.source(),
    },
)

def build_appliance_from_dir(*, name: str, dir, **kwargs):
    _build_appliance_rule(name = name, archive = dir, **(default_target_platform_kwargs() | kwargs))
