# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")

def _transition_impl(platform: PlatformInfo, refs: struct) -> PlatformInfo:
    constraints = platform.configuration.constraints

    constraints = {
        key: value
        for key, value in constraints.items()
        if not key.package.startswith("antlir")
    }

    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

_transition = transition(
    impl = _transition_impl,
    refs = {},
)

def _strip_antlir_config_impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.label.package not in [
        "antlir/antlir2/cfg/strip_antlir_config/tests",
        # @oss-disable
    ]:
        fail("""
            target is in {} which is not an allowed package.
            You MUST talk to twimage to get an exception to this rule
        """.format(ctx.label.package))
    return ctx.attrs.actual.providers

_strip_antlir_config = rule(
    impl = _strip_antlir_config_impl,
    attrs = {
        "actual": attrs.transition_dep(cfg = _transition),
        "labels": attrs.list(attrs.string(), default = []),
    },
)

strip_antlir_config = rule_with_default_target_platform(_strip_antlir_config)
