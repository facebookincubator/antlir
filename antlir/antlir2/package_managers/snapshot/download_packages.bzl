# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/package_managers/snapshot/facebook:manifold.bzl", "maybe_resolve_manifold_url")
load(":content_based_path.bzl", "should_use_content_based_path")

def _download_packages_impl(
    actions: AnalysisActions,
    packages_jsons: dict[str, ArtifactValue],
    baseurl: str,
    packages: dict[str, typing.Any],
):
    all_packages = []
    for _key, packages_json in packages_jsons.items():
        all_packages.extend(packages_json.read_json())

    for name, out in packages.items():
        matches = [p for p in all_packages if p["package"] == name]
        if len(matches) != 1:
            fail("expected exactly one package named '{}', found {}".format(name, len(matches)))
        pkg = matches[0]
        checksums = pkg["checksums"]
        url = baseurl + pkg["filename"]
        # @oss-disable[end= ]: url = maybe_resolve_manifold_url(url)

        has_content_based_path = should_use_content_based_path(actions, checksums)
        dl = actions.declare_output(
            name,
            has_content_based_path = has_content_based_path,
        )
        actions.download_file(
            dl,
            url,
            sha256 = checksums.get("sha256"),
            sha1 = checksums.get("sha1"),
        )
        actions.copy_file(out, dl)
    return []

_download_packages = dynamic_actions(
    impl = _download_packages_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "packages": dynattrs.dict(str, dynattrs.output()),
        "packages_jsons": dynattrs.dict(str, dynattrs.artifact_value()),
    },
)

def download_packages(
    *,
    actions: AnalysisActions,
    packages_jsons: dict[str, Artifact],
    baseurl: str,
    names: list[str],
    extension: str,
) -> dict[str, Artifact]:
    """Download individual packages by name from packages.json indexes.

    Searches all provided packages.json files for each name and downloads the
    matching package. Fails if there is not exactly one match for any name.
    """
    if not names:
        return {}

    outputs = {}
    for name in names:
        outputs[name] = actions.declare_output(
            "packages",
            name + extension,
        )

    actions.dynamic_output_new(
        _download_packages(
            baseurl = baseurl,
            packages_jsons = packages_jsons,
            packages = {name: out.as_output() for name, out in outputs.items()},
        ),
    )
    return outputs
