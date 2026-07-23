# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load("//antlir/antlir2/package_managers/snapshot:download_from_index.bzl", "download_from_index")
load("//antlir/antlir2/package_managers/snapshot:download_packages_subtargets.bzl", "download_packages_subtargets")
load("//antlir/antlir2/package_managers/snapshot:snapshottable.bzl", "SnapshottableInfo")
load(":repodata.bzl", "download_repodata")

YumRepoInfo = provider(
    fields = {
        "baseurl": str,
        "packages_json": Artifact,
    }
)

def _repo_impl(ctx: AnalysisContext) -> list[Provider]:
    baseurl = ctx.attrs.baseurl
    if not baseurl.endswith("/"):
        baseurl += "/"
    baseurl = baseurl.format(arch = ctx.attrs._arch)

    # If index_checksums is provided (snapshotted repo), download index.json
    # and then use it to fetch all other blobs with verified checksums via
    # dynamic deps. Otherwise fall back to the old nondeterministic repomd.xml
    # path for upstream repos.
    index_json = None
    if ctx.attrs.index_checksums:
        # Download the index.json that maps relpath → {sha1, sha256}
        index_json = download(
            actions = ctx.actions,
            out_name = "index.json",
            url = baseurl + "index.json",
            checksums = ctx.attrs.index_checksums,
            allow_nondeterministic_downloads = False,
        )

        # Download all metadata files we need for building the snapshot tree
        # and for generating canonical repomd.xml, using checksums from index.
        needed = [
            "packages.json",
            "repodata/primary.xml",
            "repodata/filelists.xml",
            "repodata/other.xml",
        ]
        from_index = download_from_index(
            actions = ctx.actions,
            index_json = index_json,
            baseurl = baseurl,
            relpaths = needed,
        )

        # For compatibility with existing generate logic, map to same struct shape
        # as download_repodata would produce.
        repodata = struct(
            packages_json = from_index["packages.json"],
            primary_xml = from_index["repodata/primary.xml"],
            filelists_xml = from_index["repodata/filelists.xml"],
            other_xml = from_index["repodata/other.xml"],
        )
    else:
        # Old path for non-snapshotted upstream repos – download repomd.xml
        # nondeterministically and parse it.
        repomd_xml = download(
            actions = ctx.actions,
            out_name = "repomd.xml",
            url = baseurl + "repodata/repomd.xml",
            metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
            checksums = {},
            allow_nondeterministic_downloads = True,
        )

        repomd_json = ctx.actions.declare_output("repomd.json")
        ctx.actions.run(
            cmd_args(
                ctx.attrs._metadata_bin[RunInfo],
                "parse",
                "yum",
                "repomd",
                repomd_xml,
                repomd_json.as_output(),
            ),
            category = "parse",
            identifier = "repomd.xml",
        )

        repodata = download_repodata(
            actions = ctx.actions,
            repomd_json = repomd_json,
            baseurl = baseurl,
            metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
        )

    # Generate a canonical `repomd.xml` that references the decompressed
    # repodata files with both sha1 + sha256 checksums (the upstream version
    # carries only sha256 and points at compressed `.gz` paths we don't
    # snapshot). The canonical version is what gets snapshotted, so the
    # next build sees it as repomd.xml.
    generated_repomd_xml = ctx.actions.declare_output(
        "generated-repomd.xml",
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs._metadata_bin[RunInfo],
            "generate",
            "yum",
            "repomd",
            cmd_args(repodata.primary_xml, format = "--primary-xml={}"),
            cmd_args(repodata.filelists_xml, format = "--filelists-xml={}"),
            cmd_args(repodata.other_xml, format = "--other-xml={}"),
            generated_repomd_xml.as_output(),
        ),
        category = "generate",
        identifier = "repomd.xml",
    )

    metadata_tree = ctx.actions.copied_dir(
        "snapshottable_metadata",
        {
            "packages.json": repodata.packages_json,
            "repodata/filelists.xml": repodata.filelists_xml,
            "repodata/other.xml": repodata.other_xml,
            "repodata/primary.xml": repodata.primary_xml,
            "repodata/repomd.xml": generated_repomd_xml,
        },
    )

    # Use the renamed download_packages_subtargets which uses the checksum index
    # and always uses content based paths.
    pkg_outputs = download_packages_subtargets(
        actions = ctx.actions,
        packages_jsons = {"packages": repodata.packages_json},
        index_json = index_json,
        baseurl = baseurl,
        names = ctx.attrs.package_subtargets,
        extension = ".rpm",
    )
    pkg_subtargets = {name + ".rpm": [DefaultInfo(out)] for name, out in pkg_outputs.items()}

    return [
        DefaultInfo(
            sub_targets = {
                "debug_only": [
                    DefaultInfo(
                        sub_targets = {
                            "packages.json": [DefaultInfo(repodata.packages_json)],
                        }
                    )
                ],
                "packages": [DefaultInfo(sub_targets = pkg_subtargets)],
            }
        ),
        YumRepoInfo(
            baseurl = baseurl,
            packages_json = repodata.packages_json,
        ),
        SnapshottableInfo(
            metadata_tree = metadata_tree,
            packages_indexes = [repodata.packages_json],
            packages_baseurl = baseurl,
            arches = ctx.attrs.arches,
            package_subtargets = ctx.attrs.package_subtargets,
            snapshot_storage = ctx.attrs.snapshot_storage,
            snapshot_buck_file = ctx.attrs.snapshot_buck_file,
        ),
    ]

_repo = rule(
    impl = _repo_impl,
    attrs = {
        "arches": attrs.list(
            attrs.string(),
            default = ["aarch64", "x86_64"],
            doc = "Architectures this repo supports (used as `{arch}` placeholder values).",
        ),
        "baseurl": attrs.string(),
        "index_checksums": attrs.dict(
            attrs.string(),
            attrs.string(),
            default = {},
            doc = "Checksum for the index file (repomd.xml) that contains sha1+sha256 of other blobs",
        ),
        "package_subtargets": attrs.list(attrs.string(), default = [], doc = "List of package names to expose as subtargets"),
        "snapshot_buck_file": attrs.option(
            attrs.string(),
            default = None,
            doc = "Optional alternate path for the generated BUCK file, relative to `fbcode/bot_generated/antlir/snapshot/` or absolute under that root, e.g. `kernel/6.13/BUCK`. Multiple repos can share the same path to be co-located in one BUCK file.",
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
        "_arch": attrs.default_only(
            attrs.string(
                default = select({
                    "ovr_config//cpu:arm64": "aarch64",
                    "ovr_config//cpu:x86_64": "x86_64",
                })
            )
        ),
        "_metadata_bin": attrs.default_only(
            attrs.exec_dep(
                providers = [RunInfo],
                default = "//antlir/antlir2/package_managers/snapshot:metadata",
            ),
        ),
    },
)

repo = rule_with_default_target_platform(_repo)
