# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load("//antlir/antlir2/package_managers/snapshot:download_from_index.bzl", "download_from_index")
load("//antlir/antlir2/package_managers/snapshot:download_packages_subtargets.bzl", "download_packages_subtargets")
load("//antlir/antlir2/package_managers/snapshot:snapshottable.bzl", "SnapshottableInfo")
load(":packages_index.bzl", "download_component_package_indexes")

ComponentInfo = provider(
    fields = {
        "arch": str,
        "name": str,
        "packages_json": Artifact,
        "packages_txt": Artifact,
    }
)

DebSuiteInfo = provider(
    fields = {
        "archive_dir": Artifact,
        "archive_url": str,
        "components": list[ComponentInfo],
        "distribution": str,
        "suite_baseurl": str,
    }
)

def _suite_impl(ctx: AnalysisContext) -> list[Provider]:
    archive_url = ctx.attrs.archive_url
    if not archive_url.endswith("/"):
        archive_url += "/"
    distribution = ctx.attrs.distribution or ctx.label.name
    suite_baseurl = archive_url + "dists/" + distribution + "/"

    # If index_checksums is provided (snapshotted repo), download index.json
    # and use it to fetch InRelease and Packages indexes via dynamic deps for determinism.
    index_json = None
    per_arch_packages_from_index = None
    if ctx.attrs.index_checksums:
        index_json = download(
            actions = ctx.actions,
            out_name = "index.json",
            url = archive_url + "index.json",
            checksums = ctx.attrs.index_checksums,
            allow_nondeterministic_downloads = False,
        )

        # Download InRelease via index
        from_index = download_from_index(
            actions = ctx.actions,
            index_json = index_json,
            baseurl = archive_url,
            relpaths = [
                "dists/{}/InRelease".format(distribution),
            ],
        )
        inrelease = from_index["dists/{}/InRelease".format(distribution)]

        # Also download Packages indexes via index for determinism when snapshotted
        needed_packages_relpaths = []
        for arch in ctx.attrs.architectures:
            for comp in ctx.attrs.components:
                needed_packages_relpaths.append("dists/{}/{}/binary-{}/Packages".format(distribution, comp, arch))
        per_arch_packages_from_index = download_from_index(
            actions = ctx.actions,
            index_json = index_json,
            baseurl = archive_url,
            relpaths = needed_packages_relpaths,
        )
    else:
        inrelease = download(
            actions = ctx.actions,
            out_name = "InRelease",
            url = suite_baseurl + "InRelease",
            metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
            checksums = {},
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
    if per_arch_packages_from_index != None:
        # Snapshotted path: Packages txt already downloaded deterministically via index.
        for arch in ctx.attrs.architectures:
            arch_map = {}
            for comp in ctx.attrs.components:
                relpath = "dists/{}/{}/binary-{}/Packages".format(distribution, comp, arch)
                txt_artifact = per_arch_packages_from_index[relpath]
                json_out = ctx.actions.declare_output(
                    "{}/{}/packages.json".format(arch, comp),
                    has_content_based_path = True,
                )
                ctx.actions.run(
                    cmd_args(
                        ctx.attrs._metadata_bin[RunInfo],
                        "parse",
                        "deb",
                        "packages",
                        txt_artifact,
                        json_out.as_output(),
                    ),
                    category = "parse",
                    identifier = "snapshotted/{}/{}/Packages".format(arch, comp),
                )
                arch_map[comp] = struct(txt = txt_artifact, json = json_out)
            per_arch_indexes[arch] = arch_map
    else:
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
    components_json = ctx.actions.write_json("components.json", components, with_inputs = True, has_content_based_path = False)

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

    archive_dir = ctx.actions.copied_dir("archive", archive_dir_srcs)
    # For snapshotting, emit `dists/...` directly at the metadata tree root
    # (without an extra `archive/` wrapper). The Rust snapshot code stores
    # files at `tree/{prefix}/{relpath}`, and `archive_url` is now
    # `tree_base_url` (no `archive` segment), so `archive_url + "dists/..."`
    # correctly resolves to the stored location. This removes the need for
    # post-hoc `archive/` stripping in the index generation.
    for path, artifact in archive_dir_srcs.items():
        metadata_tree[path] = artifact
    metadata_tree = ctx.actions.copied_dir("snapshottable_metadata", metadata_tree)

    all_packages_indexes = [p.json for arch_indexes in per_arch_indexes.values() for p in arch_indexes.values()]

    pkg_outputs = download_packages_subtargets(
        actions = ctx.actions,
        packages_jsons = {arch + "/" + cname: p.json for arch, arch_indexes in per_arch_indexes.items() for cname, p in arch_indexes.items()},
        index_json = index_json if ctx.attrs.index_checksums else None,
        baseurl = archive_url,
        names = ctx.attrs.package_subtargets,
        extension = ".deb",
    )
    pkg_subtargets = {name + ".deb": [DefaultInfo(out)] for name, out in pkg_outputs.items()}

    return [
        DefaultInfo(
            sub_targets = {
                # TODO: remove all subtargets when I'm done using them for debugging
                "debug_only": [
                    DefaultInfo(
                        sub_targets = {
                            "InRelease": [DefaultInfo(inrelease)],
                            "archive": [DefaultInfo(archive_dir)],
                            "components": [DefaultInfo(sub_targets = components_subtargets)],
                            "index.json": [DefaultInfo(index_json)],
                            "release.json": [DefaultInfo(release_json)],
                        }
                    )
                ],
                "packages": [DefaultInfo(sub_targets = pkg_subtargets)],
            }
        ),
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
            arches = ctx.attrs.architectures,
            package_subtargets = ctx.attrs.package_subtargets,
            snapshot_storage = ctx.attrs.snapshot_storage,
            snapshot_buck_file = ctx.attrs.snapshot_buck_file,
        ),
    ]

_suite = rule(
    impl = _suite_impl,
    attrs = {
        "architectures": attrs.list(
            attrs.string(),
            default = select({
                "ovr_config//cpu:arm64": ["arm64"],
                "ovr_config//cpu:x86_64": ["amd64"],
            }),
        ),
        "archive_url": attrs.string(),
        "components": attrs.list(attrs.string()),
        "distribution": attrs.option(attrs.string(), default = None),
        "index_checksums": attrs.dict(
            attrs.string(),
            attrs.string(),
            default = {},
            doc = "Checksum for the index file (InRelease) that contains sha1+sha256 of other blobs",
        ),
        "package_subtargets": attrs.list(attrs.string(), default = [], doc = "List of package names to expose as subtargets"),
        "snapshot_buck_file": attrs.option(
            attrs.string(),
            default = None,
            doc = "Optional alternate path for the generated BUCK file, relative to `fbcode/bot_generated/antlir/snapshot/` or absolute under that root.",
        ),
        "snapshot_source": attrs.option(
            attrs.label(),
            default = None,
            doc = "The original target that defined this repo snapshot",
        ),
        "snapshot_storage": attrs.option(
            attrs.dict(attrs.string(), attrs.string()),
            default = None,
            doc = 'Storage config for `snapshot run-all` (e.g. `{"type": "manifold", "bucket": "...", "api_key": "..."}`).',
        ),
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
