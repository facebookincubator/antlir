# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")

DebSuiteInfo = provider(fields = {
    "archive_url": str,
    "distribution": str,
    "suite_baseurl": str,
})

def _suite_impl(ctx: AnalysisContext) -> list[Provider]:
    archive_url = ctx.attrs.archive_url
    if not archive_url.endswith("/"):
        archive_url += "/"
    distribution = ctx.attrs.distribution or ctx.label.name
    suite_baseurl = archive_url + "dists/" + distribution + "/"

    inrelease = download(
        actions = ctx.actions,
        out_name = "InRelease",
        url = suite_baseurl + "InRelease",
        snapshot_run_info = ctx.attrs._snapshot_bin[RunInfo],
        checksums = ctx.attrs.inrelease_checksums,
        allow_nondeterministic_downloads = True,
    )

    release_json = ctx.actions.declare_output("release.json", has_content_based_path = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._snapshot_bin[RunInfo],
            "parse",
            "deb",
            "release",
            inrelease,
            release_json.as_output(),
        ),
        category = "parse",
        identifier = "InRelease",
    )

    return [
        DefaultInfo(sub_targets = {
            # TODO: remove all subtargets when I'm done using them for debugging
            "debug_only": [DefaultInfo(sub_targets = {
                "release.json": [DefaultInfo(release_json)],
            })],
        }),
        DebSuiteInfo(
            archive_url = archive_url,
            distribution = distribution,
            suite_baseurl = suite_baseurl,
        ),
    ]

_suite = rule(
    impl = _suite_impl,
    attrs = {
        "archive_url": attrs.string(),
        "distribution": attrs.option(attrs.string(), default = None),
        "inrelease_checksums": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "_snapshot_bin": attrs.default_only(
            attrs.exec_dep(
                providers = [RunInfo],
                default = "//antlir/antlir2/package_managers/snapshot:snapshot",
            ),
        ),
    },
)

suite = rule_with_default_target_platform(_suite)
