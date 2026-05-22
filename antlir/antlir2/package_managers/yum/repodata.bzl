# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")

def _download_repodata_impl(
    actions: AnalysisActions, repomd_json: ArtifactValue, primary_xml, filelists_xml, other_xml, packages_json, baseurl: str, metadata_run_info: RunInfo
):
    repomd = repomd_json.read_json()

    for name, out in [("primary", primary_xml), ("filelists", filelists_xml), ("other", other_xml)]:
        blob = repomd[name]
        download(
            actions = actions,
            out = out,
            url = baseurl + blob["href"],
            checksums = blob["checksums"],
            metadata_run_info = metadata_run_info,
        )

    actions.run(
        cmd_args(
            metadata_run_info,
            "parse",
            "yum",
            "primary",
            "--primary-xml",
            primary_xml.as_input(),
            "--basic-out",
            packages_json,
        ),
        category = "parse",
        identifier = "primary.xml",
    )

    return []

_download_repodata = dynamic_actions(
    impl = _download_repodata_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "filelists_xml": dynattrs.output(),
        "metadata_run_info": dynattrs.value(RunInfo),
        "other_xml": dynattrs.output(),
        "packages_json": dynattrs.output(),
        "primary_xml": dynattrs.output(),
        "repomd_json": dynattrs.artifact_value(),
    },
)

def download_repodata(*, actions: AnalysisActions, repomd_json: Artifact, baseurl: str, metadata_run_info: RunInfo) -> struct:
    primary_xml = actions.declare_output(
        "primary.xml",
        has_content_based_path = True,
    )
    filelists_xml = actions.declare_output(
        "filelists.xml",
        has_content_based_path = True,
    )
    other_xml = actions.declare_output(
        "other.xml",
        has_content_based_path = True,
    )
    packages_json = actions.declare_output(
        "packages.json",
        has_content_based_path = True,
    )

    actions.dynamic_output_new(
        _download_repodata(
            baseurl = baseurl,
            metadata_run_info = metadata_run_info,
            repomd_json = repomd_json,
            primary_xml = primary_xml.as_output(),
            filelists_xml = filelists_xml.as_output(),
            other_xml = other_xml.as_output(),
            packages_json = packages_json.as_output(),
        ),
    )
    return struct(
        packages_json = packages_json,
        primary_xml = primary_xml,
        filelists_xml = filelists_xml,
        other_xml = other_xml,
    )
