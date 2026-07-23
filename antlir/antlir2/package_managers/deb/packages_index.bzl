# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/package_managers/snapshot:download.bzl", "download")

def _download_packages_indexes_impl(
    actions: AnalysisActions, arch: str, release_json: ArtifactValue, packages_indexes: dict[str, typing.Any], suite_baseurl: str, metadata_run_info: RunInfo
):
    release = release_json.read_json()

    for component_name, (txt, json) in packages_indexes.items():
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

    return []

_download_packages_indexes = dynamic_actions(
    impl = _download_packages_indexes_impl,
    attrs = {
        "arch": dynattrs.value(str),
        "metadata_run_info": dynattrs.value(RunInfo),
        "packages_indexes": dynattrs.dict(
            str,
            dynattrs.tuple(
                dynattrs.output(),
                dynattrs.output(),
            ),
        ),
        "release_json": dynattrs.artifact_value(),
        "suite_baseurl": dynattrs.value(str),
    },
)

ComponentPackages = record(
    txt = Artifact,
    json = Artifact,
)

def download_component_package_indexes(
    *, actions: AnalysisActions, components: list[str], arch: str, suite_baseurl: str, release_json: Artifact, metadata_run_info: RunInfo
) -> dict[str, ComponentPackages]:
    components = {
        component: ComponentPackages(
            txt = actions.declare_output(arch + "/" + component, "Packages", has_content_based_path = False),
            json = actions.declare_output(arch + "/" + component, "packages.json", has_content_based_path = False),
        )
        for component in components
    }

    actions.dynamic_output_new(
        _download_packages_indexes(
            arch = arch,
            metadata_run_info = metadata_run_info,
            suite_baseurl = suite_baseurl,
            release_json = release_json,
            packages_indexes = {component_name: (out.txt.as_output(), out.json.as_output()) for component_name, out in components.items()},
        ),
    )
    return components
