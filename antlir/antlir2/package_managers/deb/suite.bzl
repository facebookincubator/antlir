# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load(":packages_index.bzl", "download_component_package_indexes")

ComponentInfo = provider(fields = {
    "name": str,
    "packages_json": Artifact,
    "packages_txt": Artifact,
})

DebSuiteInfo = provider(fields = {
    "archive_url": str,
    "components": list[ComponentInfo],
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

    components_package_indexes = download_component_package_indexes(
        actions = ctx.actions,
        release_json = release_json,
        components = ctx.attrs.components,
        arch = ctx.attrs._arch,
        suite_baseurl = suite_baseurl,
        snapshot_run_info = ctx.attrs._snapshot_bin[RunInfo],
    )
    components = [
        ComponentInfo(name = c, packages_json = p.json, packages_txt = p.txt)
        for c, p in components_package_indexes.items()
    ]

    components_subtargets = {}
    for cname, c in components_package_indexes.items():
        components_subtargets[cname] = [DefaultInfo(sub_targets = {"packages.json": [DefaultInfo(c.json)]})]

    return [
        DefaultInfo(sub_targets = {
            # TODO: remove all subtargets when I'm done using them for debugging
            "debug_only": [DefaultInfo(sub_targets = {
                "components": [DefaultInfo(sub_targets = components_subtargets)],
                "release.json": [DefaultInfo(release_json)],
            })],
        }),
        DebSuiteInfo(
            archive_url = archive_url,
            distribution = distribution,
            components = components,
            suite_baseurl = suite_baseurl,
        ),
    ]

_suite = rule(
    impl = _suite_impl,
    attrs = {
        "archive_url": attrs.string(),
        "components": attrs.list(attrs.string()),
        "distribution": attrs.option(attrs.string(), default = None),
        "inrelease_checksums": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "_arch": attrs.default_only(attrs.string(default = select({
            "ovr_config//cpu:arm64": "arm64",
            "ovr_config//cpu:x86_64": "amd64",
        }))),
        "_snapshot_bin": attrs.default_only(
            attrs.exec_dep(
                providers = [RunInfo],
                default = "//antlir/antlir2/package_managers/snapshot:snapshot",
            ),
        ),
    },
)

suite = rule_with_default_target_platform(_suite)
