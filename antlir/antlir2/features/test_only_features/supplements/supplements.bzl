# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "PlanInfo", "Planner")

def supplements(*, msg: str):
    return ParseTimeFeature(
        feature_type = "test_only_features/supplements",
        plugin = "antlir//antlir/antlir2/features/test_only_features/supplements:supplements",
        kwargs = {
            "msg": msg,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    msg = ctx.attrs.msg

    def _mutate_supplements(supplements: dict[str, typing.Any]) -> dict[str, typing.Any]:
        supplements["msgs"] = list(supplements.get("msgs", []))
        supplements["msgs"].append(msg)
        return supplements

    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "test_only_features/supplements",
            data = struct(),
            plugin = ctx.attrs.plugin,
            mutate_supplements = _mutate_supplements,
            planner = Planner(
                fn = _plan_fn,
                kwargs = {"msg": msg},
            ),
        ),
    ]

supplements_rule = rule(
    impl = _impl,
    attrs = {
        "msg": attrs.string(),
        "plugin": attrs.label(),
    },
)

def _plan_fn(*, ctx: AnalysisContext, msg: str, **_kwargs) -> list[PlanInfo]:
    def _mutate_supplements(supplements: dict[str, typing.Any]) -> dict[str, typing.Any]:
        supplements["planner_msgs"] = list(supplements.get("planner_msgs", []))
        supplements["planner_msgs"].append(msg)
        return supplements

    return [
        PlanInfo(
            id = "supplements",
            mutate_supplements = _mutate_supplements,
        ),
    ]
