# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    supplements_json = ctx.actions.write_json("supplements.json", ctx.attrs.layer[LayerInfo].supplements, has_content_based_path = False)
    return [
        DefaultInfo(supplements_json),
    ]

_dump_supplements = rule(
    impl = _impl,
    attrs = {
        "layer": attrs.dep(providers = [LayerInfo]),
    },
)

dump_supplements = rule_with_default_target_platform(_dump_supplements)
