# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load("//antlir/antlir2/package_managers/snapshot:download_from_index.bzl", "download_from_index")
load("//antlir/antlir2/package_managers/snapshot:download_packages_subtargets.bzl", "download_members", "download_packages_subtargets")
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
        "base_variant": str,
        "components": list[ComponentInfo],
        # Pre-built directory of .deb files keyed by Filename, used instead of
        # downloading from archive_url
        "debs_dir": provider_field(Artifact | None, default = None),
        "distribution": str,
        # Resolver for a single .deb: `(actions, pkg) -> Artifact`, where
        # `pkg` is a resolved package dict from the apt transaction. Backed
        # by `_suite_get_deb`; consumers call this instead of branching on
        # how the suite exports its .debs.
        "get_deb": provider_field(typing.Any),
        # Member .deb artifacts keyed by `{name}_{version}_{arch}`,
        # covering every package in the suite.
        "members": dict[str, Artifact],
        "suite_baseurl": str,
    }
)

def _suite_get_deb(
    actions: AnalysisActions,
    archive_url: str,
    members: dict[str, Artifact],
    pkg: dict[str, typing.Any],
    metadata_run_info: RunInfo,
) -> Artifact:
    """Backing implementation for the `get_deb` provider field: resolve a
    single .deb for a resolved apt transaction package.

    Suites with `members` reuse those artifacts (registered once on the
    repo target and shared by every image); suites without them download
    inline with checksum verification.
    """
    filename = pkg["filename"]
    if members:
        nevra = "{}_{}_{}".format(pkg["name"], pkg["version"], pkg["arch"])
        member = members.get(nevra)
        if member == None:
            fail(
                "package '{}' (filename '{}') is not in the suite members, the suite snapshot is stale".format(
                    nevra,
                    filename,
                )
            )
        return member
    sha256 = pkg.get("sha256") or None
    sha512 = pkg.get("sha512") or None
    checksums = {}
    if sha256:
        checksums["sha256"] = sha256
    elif sha512:
        checksums["sha512"] = sha512
    return download(
        actions = actions,
        url = archive_url + filename,
        out_name = filename,
        checksums = checksums,
        allow_nondeterministic_downloads = not checksums,
        metadata_run_info = metadata_run_info,
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
            out = ctx.actions.declare_output("index.json", has_content_based_path = False).as_output(),
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

    # Package aliases expose individual .debs by name (first match wins),
    # using the checksum index with content based paths.
    aliases = sorted({a: True for a in ctx.attrs.package_subtargets + ctx.attrs.package_aliases_subtargets}.keys())
    alias_outputs = download_packages_subtargets(
        actions = ctx.actions,
        packages_jsons = {arch + "/" + cname: p.json for arch, arch_indexes in per_arch_indexes.items() for cname, p in arch_indexes.items()},
        index_json = index_json if ctx.attrs.index_checksums else None,
        baseurl = archive_url,
        names = aliases,
        extension = ".deb",
        metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
    )
    alias_subtargets = {name + ".deb": [DefaultInfo(out)] for name, out in alias_outputs.items()}

    # Members expose every package in the suite by
    # `{name}_{version}_{arch}`. Unlike yum, one suite analysis covers all
    # architectures against a single archive url, so every member is
    # downloadable here.
    member_outputs = download_members(
        actions = ctx.actions,
        packages_jsons = {arch + "/" + cname: p.json for arch, arch_indexes in per_arch_indexes.items() for cname, p in arch_indexes.items()},
        index_json = index_json if ctx.attrs.index_checksums else None,
        baseurl = archive_url,
        nevras = ctx.attrs.members,
    )
    member_subtargets = {nevra: [DefaultInfo(out)] for nevra, out in member_outputs.items()}

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
                "package_aliases_subtargets": [DefaultInfo(sub_targets = alias_subtargets)],
                "packages": [DefaultInfo(sub_targets = member_subtargets)],
            }
        ),
        DebSuiteInfo(
            archive_dir = archive_dir,
            archive_url = archive_url,
            base_variant = ctx.attrs.base_variant,
            distribution = distribution,
            components = components,
            get_deb = lambda actions, pkg, _md = ctx.attrs._metadata_bin[RunInfo]: _suite_get_deb(actions, archive_url, member_outputs, pkg, _md),
            members = member_outputs,
            suite_baseurl = suite_baseurl,
        ),
        SnapshottableInfo(
            metadata_tree = metadata_tree,
            packages_indexes = all_packages_indexes,
            packages_baseurl = archive_url,
            arches = ctx.attrs.architectures,
            package_subtargets = aliases,
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
        "base_variant": attrs.enum(
            ["none", "minbase", "standard"],
            default = "minbase",
            doc = "Seed the debootstrap base set (Essential + Required for minbase, plus Important for standard) into every installing transaction. Seeded packages are marked before user items, so explicit removals still win.",
        ),
        "components": attrs.list(attrs.string()),
        "distribution": attrs.option(attrs.string(), default = None),
        "index_checksums": attrs.dict(
            attrs.string(),
            attrs.string(),
            default = {},
            doc = "Checksum for the index file (InRelease) that contains sha1+sha256 of other blobs",
        ),
        "members": attrs.list(
            attrs.string(),
            default = [],
            doc = "Full `{name}_{version}_{arch}` NEVRAs: arch-independent members plus (via select) single-arch members for the current arch. The snapshot binary emits this as a plain list plus a per-arch select(); the rule downloads all members (via the index) and exposes them as `packages` subtargets and in `DebSuiteInfo`.",
        ),
        "package_aliases_subtargets": attrs.list(attrs.string(), default = [], doc = "List of package names to expose as `package_aliases_subtargets`"),
        "package_subtargets": attrs.list(attrs.string(), default = [], doc = "Deprecated alias for `package_aliases_subtargets`"),
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
