# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load("//antlir/antlir2/package_managers/snapshot:snapshottable.bzl", "SnapshottableInfo")
load(":packages_index.bzl", "download_component_package_indexes")

ComponentInfo = provider(fields = {
    "arch": str,
    "name": str,
    "packages_json": Artifact,
    "packages_txt": Artifact,
})

DebSuiteInfo = provider(fields = {
    "archive_dir": Artifact,
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
        metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
        checksums = ctx.attrs.inrelease_checksums,
        allow_nondeterministic_downloads = True,
    )

    metadata_tree = {}

    release_json = ctx.actions.declare_output("release.json", has_content_based_path = True)
    metadata_tree["release.json"] = release_json
    ctx.actions.run(
        cmd_args(
            ctx.attrs._metadata_bin[RunInfo],
            "parse",
            "deb",
            "release",
            inrelease,
            release_json.as_output(),
        ),
        category = "parse",
        identifier = "InRelease",
    )

    # Download package indexes for each architecture
    per_arch_indexes = {}
    for arch in ctx.attrs.architectures:
        per_arch_indexes[arch] = download_component_package_indexes(
            actions = ctx.actions,
            release_json = release_json,
            components = ctx.attrs.components,
            arch = arch,
            suite_baseurl = suite_baseurl,
            metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
        )

    components = [
        ComponentInfo(name = c, arch = arch, packages_json = p.json, packages_txt = p.txt)
        for arch, arch_indexes in per_arch_indexes.items()
        for c, p in arch_indexes.items()
    ]
    components_json = ctx.actions.write_json("components.json", components, with_inputs = True)

    components_subtargets = {}
    for arch, arch_indexes in per_arch_indexes.items():
        for cname, c in arch_indexes.items():
            key = arch + "/" + cname
            components_subtargets[key] = [DefaultInfo(sub_targets = {"packages.json": [DefaultInfo(c.json)]})]
            metadata_tree["components/" + key + "/packages.json"] = c.json

    # Generate a custom InRelease that only lists the components we care
    # about, signed with a dummy key so apt can verify it.
    generated_inrelease = ctx.actions.declare_output(
        "generated-InRelease",
        has_content_based_path = True,
    )
    generate_cmd = cmd_args(
        ctx.attrs._metadata_bin[RunInfo],
        "generate",
        "deb",
        "inrelease",
        cmd_args(release_json, format = "--release-json={}"),
        cmd_args(ctx.attrs._signing_key, format = "--signing-key={}"),
        cmd_args(components_json, format = "--components-json={}"),
        generated_inrelease.as_output(),
    )
    ctx.actions.run(
        generate_cmd,
        category = "generate",
        identifier = "InRelease",
    )

    dist_prefix = "dists/" + distribution + "/"
    archive_dir_srcs = {
        dist_prefix + "InRelease": generated_inrelease,
    }
    for arch, arch_indexes in per_arch_indexes.items():
        for cname, c in arch_indexes.items():
            archive_dir_srcs[dist_prefix + cname + "/binary-" + arch + "/Packages"] = c.txt

    archive_dir = ctx.actions.symlinked_dir("archive", archive_dir_srcs, has_content_based_path = False)
    metadata_tree["archive"] = archive_dir
    metadata_tree = ctx.actions.symlinked_dir("snapshottable_metadata", metadata_tree, has_content_based_path = False)

    all_packages_indexes = [
        p.json
        for arch_indexes in per_arch_indexes.values()
        for p in arch_indexes.values()
    ]

    return [
        DefaultInfo(sub_targets = {
            # TODO: remove all subtargets when I'm done using them for debugging
            "debug_only": [DefaultInfo(sub_targets = {
                "archive": [DefaultInfo(archive_dir)],
                "components": [DefaultInfo(sub_targets = components_subtargets)],
                "release.json": [DefaultInfo(release_json)],
            })],
        }),
        DebSuiteInfo(
            archive_dir = archive_dir,
            archive_url = archive_url,
            distribution = distribution,
            components = components,
            suite_baseurl = suite_baseurl,
        ),
        SnapshottableInfo(
            metadata_tree = metadata_tree,
            packages_indexes = all_packages_indexes,
            packages_baseurl = archive_url,
        ),
    ]

_suite = rule(
    impl = _suite_impl,
    attrs = {
        "architectures": attrs.list(attrs.string(), default = select({
            "ovr_config//cpu:arm64": ["arm64"],
            "ovr_config//cpu:x86_64": ["amd64"],
        })),
        "archive_url": attrs.string(),
        "components": attrs.list(attrs.string()),
        "distribution": attrs.option(attrs.string(), default = None),
        "inrelease_checksums": attrs.dict(attrs.string(), attrs.string(), default = {}),
        "_metadata_bin": attrs.default_only(
            attrs.exec_dep(
                providers = [RunInfo],
                default = "//antlir/antlir2/package_managers/snapshot:metadata",
            ),
        ),
        "_signing_key": attrs.default_only(
            attrs.source(
                default = "//antlir/antlir2/package_managers/deb:dummy_signing_key",
            ),
        ),
    },
)

suite = rule_with_default_target_platform(_suite)
