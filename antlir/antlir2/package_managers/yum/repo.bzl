# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")
load("//antlir/antlir2/package_managers/snapshot:download_packages.bzl", "download_packages")
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
    if "{arch}" not in baseurl:
        fail("baseurl '{}' does not contain {{arch}} placeholder".format(baseurl))
    baseurl = baseurl.format(arch = ctx.attrs._arch)

    # Download repomd.xml
    repomd_xml = download(
        actions = ctx.actions,
        out_name = "repomd.xml",
        url = baseurl + "repodata/repomd.xml",
        metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
        checksums = ctx.attrs.repomd_checksums,
        allow_nondeterministic_downloads = True,
    )

    # Parse repomd.xml to get primary file location
    repomd_json = ctx.actions.declare_output("repomd.json", has_content_based_path = True)
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

    # Download and parse all repodata
    repodata = download_repodata(
        actions = ctx.actions,
        repomd_json = repomd_json,
        baseurl = baseurl,
        metadata_run_info = ctx.attrs._metadata_bin[RunInfo],
    )

    # Build metadata tree for snapshotting
    metadata_tree = ctx.actions.symlinked_dir(
        "snapshottable_metadata",
        {
            "packages.json": repodata.packages_json,
            "repodata/filelists.xml": repodata.filelists_xml,
            "repodata/other.xml": repodata.other_xml,
            "repodata/primary.xml": repodata.primary_xml,
            "repodata/repomd.xml": repomd_xml,
        },
    )

    pkg_outputs = download_packages(
        actions = ctx.actions,
        packages_jsons = {"packages": repodata.packages_json},
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
                            "repomd.json": [DefaultInfo(repomd_json)],
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
        ),
    ]

_repo = rule(
    impl = _repo_impl,
    attrs = {
        "baseurl": attrs.string(),
        "package_subtargets": attrs.list(attrs.string(), default = [], doc = "List of package names to expose as subtargets"),
        "repomd_checksums": attrs.dict(attrs.string(), attrs.string(), default = {}),
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
