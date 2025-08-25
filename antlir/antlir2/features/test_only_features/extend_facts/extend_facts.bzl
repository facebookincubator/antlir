# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "PlanInfo", "Planner")

def extend_facts(
        *,
        msg: str):
    return ParseTimeFeature(
        feature_type = "test_only_features/extend_facts",
        plugin = "antlir//antlir/antlir2/features/test_only_features/extend_facts:extend_facts",
        kwargs = {
            "msg": msg,
        },
    )

def _fact(msg: str) -> struct:
    return struct(
        type = "test_appears_in_facts::ExtendFacts",
        key = msg,
        value = struct(
            msg = msg,
        ),
    )

def _plan_fn(*, ctx: AnalysisContext, identifier: str, msg: str, **_kwargs) -> list[PlanInfo]:
    out = ctx.actions.declare_output(identifier, "out")
    fact = ctx.actions.write_json(out, [_fact("planner: " + msg)])
    return [PlanInfo(
        id = "extend_facts",
        hidden = [],
        output = fact,
        extend_facts_json = [fact],
    )]

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json("facts.json", [_fact(ctx.attrs.msg)])

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                msg = ctx.attrs.msg,
            ),
            feature_type = "test_only_features/extend_facts",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
            planner = Planner(
                fn = _plan_fn,
                kwargs = {"msg": ctx.attrs.msg},
            ),
        ),
    ]

extend_facts_rule = rule(
    impl = _impl,
    attrs = {
        "msg": attrs.string(),
        "plugin": attrs.label(),
    },
)
