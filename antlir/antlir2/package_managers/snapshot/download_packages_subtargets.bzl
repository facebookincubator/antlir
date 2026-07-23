# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/package_managers/snapshot/facebook:manifold.bzl", "maybe_resolve_manifold_url")

def _download_packages_subtargets_impl(
    actions: AnalysisActions,
    packages_jsons: dict[str, ArtifactValue],
    index_json: ArtifactValue | None,
    baseurl: str,
    packages: dict[str, typing.Any],
):
    all_packages = []
    for _key, packages_json in packages_jsons.items():
        all_packages.extend(packages_json.read_json())

    index = None
    if index_json != None:
        index = index_json.read_json()

    for name, out in packages.items():
        matches = [p for p in all_packages if p["package"] == name]
        if len(matches) != 1:
            fail("expected exactly one package named '{}', found {}".format(name, len(matches)))
        pkg = matches[0]
        filename = pkg["filename"]

        if index != None:
            # Use checksums from index.json which contains both sha1+sha256 for every file.
            checksums = index.get(filename)
            if checksums == None:
                fail(
                    "no checksums for '{}' (filename '{}') in index".format(
                        name,
                        filename,
                    )
                )
        else:
            # Fallback for upstream repos without index – use checksums from packages.json
            checksums = pkg["checksums"]

        url = baseurl + filename
        # @oss-disable[end= ]: url = maybe_resolve_manifold_url(url)

        # Always use content based paths for package subtargets – they are
        # content-addressed via sha256.
        dl = actions.declare_output(
            name,
            has_content_based_path = True,
        )
        actions.download_file(
            dl,
            url,
            sha256 = checksums.get("sha256"),
            sha1 = checksums.get("sha1"),
        )
        actions.copy_file(out, dl)
    return []

_download_packages_subtargets = dynamic_actions(
    impl = _download_packages_subtargets_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "index_json": dynattrs.option(dynattrs.artifact_value()),
        "packages": dynattrs.dict(str, dynattrs.output()),
        "packages_jsons": dynattrs.dict(str, dynattrs.artifact_value()),
    },
)

def download_packages_subtargets(
    *,
    actions: AnalysisActions,
    packages_jsons: dict[str, Artifact],
    index_json: Artifact | None = None,
    baseurl: str,
    names: list[str],
    extension: str,
) -> dict[str, Artifact]:
    """Download individual packages by name using the checksum index.

    Searches all provided packages.json files for each name to get its filename,
    then looks up checksums for that filename in index_json if provided,
    otherwise falls back to checksums from packages.json.
    Always uses content based paths.

    This is distinct from packages downloaded as part of normal package-manager
    dependency resolution – it is for the `package_subtargets` feature that
    exposes individual .rpm/.deb files as Buck subtargets.
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
        _download_packages_subtargets(
            baseurl = baseurl,
            index_json = index_json,
            packages_jsons = packages_jsons,
            packages = {name: out.as_output() for name, out in outputs.items()},
        ),
    )
    return outputs
