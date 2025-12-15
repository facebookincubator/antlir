# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# WARNING: THAR BE DRAGONS
# DO NOT USE THIS UNLESS YOU *REALLY* KNOW WHAT YOU ARE DOING.
#
# Define a transition that strips *ALL* platform configuration, which can be
# used to reduce some duplicate actions where we know that certain rules will
# never use `select`-ed attrs based on the target platform (exec platform
# configuration is still preserved).
#
# This works because buck2 caches artifacts based on the hash of the *entire*
# target platform configuration and does not know what parts are actually used.
#
# This is useful almost exclusively for antlir2 rules that exist very low in the
# dependency chain where leaf rules will have often applied their own
# configuration that we know will be irrelevant - prime example is the `rpm`
# snapshot rules - the output of that rule is always 100% identical no matter
# what the platform configuration may be, so that work should be shared.

def _strip_configuration_impl(ctx: AnalysisContext) -> list[Provider]:
    plat = ctx.attrs._stripped_platform[PlatformInfo]
    return [
        DefaultInfo(),
        TransitionInfo(impl = lambda platform: plat),
    ]

strip_configuration = rule(
    impl = _strip_configuration_impl,
    attrs = {
        "_stripped_platform": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/cfg/strip_configuration:empty-platform")),
    },
    is_configuration_rule = True,
)

def _strip_configuration_alias_impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.label.package not in [
        "antlir/antlir2/cfg/strip_configuration/tests",
        # @oss-disable[end= ]: "ti/platform/edgeos/base_image/rootfs/features/root_password",
    ]:
        fail("""
            target is in {} which is not an allowed package.
            You MUST talk to twimage to get an exception to this rule
        """.format(ctx.label.package))
    return ctx.attrs.actual.providers

strip_configuration_alias = rule(
    impl = _strip_configuration_alias_impl,
    attrs = {
        "actual": attrs.transition_dep(cfg = "antlir//antlir/antlir2/cfg/strip_configuration:strip-configuration"),
        "labels": attrs.list(attrs.string(), default = []),
    },
)
