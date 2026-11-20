# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":suite.bzl", "DebSuiteInfo")

DebSuiteSetInfo = provider(
    fields = {
        # One archive with every suite's dists/<distribution>, which apt sees as
        # one source per distribution
        "archive_dir": Artifact,
        "base_variant": str,
        # DebSuiteInfo by distribution
        "suites": dict[str, typing.Any],
    }
)

def suite_set_info(dep: Dependency) -> DebSuiteSetInfo:
    """A `suite_set`'s suites, or a single suite as a set of one."""
    if DebSuiteSetInfo in dep:
        return dep[DebSuiteSetInfo]
    suite = dep[DebSuiteInfo]
    return DebSuiteSetInfo(
        archive_dir = suite.archive_dir,
        base_variant = suite.base_variant,
        suites = {suite.distribution: suite},
    )

def _suite_set_impl(ctx: AnalysisContext) -> list[Provider]:
    suites = {}
    for dep in ctx.attrs.suites:
        suite = dep[DebSuiteInfo]
        if suite.distribution in suites:
            fail("two suites provide distribution '{}'".format(suite.distribution))
        suites[suite.distribution] = suite

    archive_dir = ctx.actions.symlinked_dir(
        "archive",
        {"dists/" + dist: suite.archive_dir.project("dists/" + dist) for dist, suite in suites.items()},
        has_content_based_path = False,
    )

    return [
        DefaultInfo(archive_dir),
        DebSuiteSetInfo(
            archive_dir = archive_dir,
            base_variant = ctx.attrs.base_variant or ctx.attrs.suites[0][DebSuiteInfo].base_variant,
            suites = suites,
        ),
    ]

_suite_set = rule(
    impl = _suite_set_impl,
    attrs = {
        "base_variant": attrs.option(
            attrs.enum(["none", "essential", "minbase", "standard"]),
            default = None,
            doc = "seeds the transaction; defaults to the first suite's",
        ),
        "suites": attrs.list(attrs.dep(providers = [DebSuiteInfo])),
    },
    doc = "Suites an apt feature resolves across, as apt does across its sources",
)

suite_set = rule_with_default_target_platform(_suite_set)
