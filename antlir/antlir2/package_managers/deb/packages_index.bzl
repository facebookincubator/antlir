# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")

def _download_indexes_impl(
    actions: AnalysisActions, arch: str, release_json: ArtifactValue, indexes: dict[str, typing.Any], suite_baseurl: str, metadata_run_info: RunInfo
):
    release = release_json.read_json()

    for component_name, (txt, json, contents, sonames) in indexes.items():
        comp_data = release["components"].get(component_name)
        if comp_data == None:
            fail("component '{}' not found in Release file".format(component_name))
        pkg_info = comp_data["packages"].get(arch)
        if pkg_info == None:
            fail("architecture '{}' not found for component '{}'".format(arch, component_name))
        pkg_info = struct(**pkg_info)

        download(
            actions = actions,
            out = txt,
            url = suite_baseurl + pkg_info.href,
            checksums = pkg_info.checksums,
            metadata_run_info = metadata_run_info,
        )

        actions.run(
            cmd_args(
                metadata_run_info,
                "parse",
                "deb",
                "packages",
                txt.as_input(),
                json,
            ),
            category = "parse",
            identifier = "{}/{}/Packages".format(arch, component_name),
        )

        contents_info = comp_data["contents"].get(arch)
        if contents_info == None:
            # Contents is optional in the archive format. Without it nothing in
            # this component can be found by soname, but naming packages
            # explicitly still works, so this is not fatal.
            actions.write(contents, "")
            actions.write_json(sonames, {})
            continue
        contents_info = struct(**contents_info)

        # Kept exactly as published (gzipped, in practice) rather than
        # decompressed like Packages: apt never reads it, so the only consumer
        # is the parse below, and 170MiB of text is not worth snapshotting.
        download(
            actions = actions,
            out = contents,
            url = suite_baseurl + contents_info.href,
            checksums = contents_info.checksums,
            decompress = False,
            metadata_run_info = metadata_run_info,
        )

        # The soname -> packages map the apt resolver looks sonames up in:
        # Debian publishes no soname metadata in the package index, so the
        # archive's file list is the only thing that can answer it.
        actions.run(
            cmd_args(metadata_run_info, "parse", "deb", "contents", contents.as_input(), sonames),
            category = "parse",
            identifier = "{}/{}/Contents".format(arch, component_name),
        )

    return []

_download_indexes = dynamic_actions(
    impl = _download_indexes_impl,
    attrs = {
        "arch": dynattrs.value(str),
        "indexes": dynattrs.dict(
            str,
            dynattrs.tuple(
                dynattrs.output(),
                dynattrs.output(),
                dynattrs.output(),
                dynattrs.output(),
            ),
        ),
        "metadata_run_info": dynattrs.value(RunInfo),
        "release_json": dynattrs.artifact_value(),
        "suite_baseurl": dynattrs.value(str),
    },
)

ComponentPackages = record(
    txt = Artifact,
    json = Artifact,
    contents = Artifact,
    sonames = Artifact,
)

def download_component_package_indexes(
    *, actions: AnalysisActions, components: list[str], arch: str, suite_baseurl: str, release_json: Artifact, metadata_run_info: RunInfo
) -> dict[str, ComponentPackages]:
    components = {
        component: ComponentPackages(
            txt = actions.declare_output(arch + "/" + component, "Packages", has_content_based_path = False),
            json = actions.declare_output(arch + "/" + component, "packages.json", has_content_based_path = False),
            contents = actions.declare_output(arch + "/" + component, "Contents", has_content_based_path = False),
            sonames = actions.declare_output(arch + "/" + component, "sonames.json", has_content_based_path = False),
        )
        for component in components
    }

    actions.dynamic_output_new(
        _download_indexes(
            arch = arch,
            metadata_run_info = metadata_run_info,
            suite_baseurl = suite_baseurl,
            release_json = release_json,
            indexes = {
                component_name: (out.txt.as_output(), out.json.as_output(), out.contents.as_output(), out.sonames.as_output())
                for component_name, out in components.items()
            },
        ),
    )
    return components
