# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/package_managers/snapshot/facebook:manifold.bzl", "maybe_resolve_manifold_url")
load(":download.bzl", "download")

def _download_packages_subtargets_impl(
    actions: AnalysisActions,
    packages_jsons: dict[str, ArtifactValue],
    index_json: ArtifactValue | None,
    baseurl: str,
    packages: dict[str, typing.Any],
    metadata_run_info: RunInfo,
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
        # content-addressed (sha256 when available, otherwise the verified
        # download below still pins the bytes).
        dl = actions.declare_output(
            name,
            has_content_based_path = True,
        )
        fetched = download(
            actions = actions,
            url = url,
            out = dl,
            checksums = checksums,
            metadata_run_info = metadata_run_info,
        )
        actions.copy_file(out, fetched)
    return []

_download_packages_subtargets = dynamic_actions(
    impl = _download_packages_subtargets_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "index_json": dynattrs.option(dynattrs.artifact_value()),
        "metadata_run_info": dynattrs.value(RunInfo),
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
    metadata_run_info: RunInfo,
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
            metadata_run_info = metadata_run_info,
            packages_jsons = packages_jsons,
            packages = {name: out.as_output() for name, out in outputs.items()},
        ),
    )
    return outputs

def _member_nevra(pkg: dict[str, typing.Any]) -> str:
    # Must match Package::nevra in
    # //antlir/antlir2/package_managers/snapshot:snapshot_common, which is
    # what the snapshot binary stores in the `members` attr. Deliberately
    # one format (`{name}_{version}_{arch}`) for yum and deb alike.
    return "{}_{}_{}".format(pkg["package"], pkg["version"], pkg["architecture"])

def _download_members_impl(
    actions: AnalysisActions,
    packages_jsons: dict[str, ArtifactValue],
    index_json: ArtifactValue | None,
    baseurl: str,
    members: dict[str, typing.Any],
):
    # Index packages by nevra once: the same nevra may be listed by
    # several indexes (e.g. a deb visible under multiple components);
    # they all point at the same file, so first match wins. Scanning the
    # full list per member is quadratic and dominates dynamic analysis
    # time on large suites.
    by_nevra = {}
    for _key, packages_json in packages_jsons.items():
        for pkg in packages_json.read_json():
            nevra = _member_nevra(pkg)
            if nevra not in by_nevra:
                by_nevra[nevra] = pkg

    index = None
    if index_json != None:
        index = index_json.read_json()

    for nevra, out in members.items():
        pkg = by_nevra.get(nevra)
        if pkg == None:
            fail("no package found for member '{}'".format(nevra))
        filename = pkg["filename"]

        if index != None:
            # Use checksums from index.json which contains both sha1+sha256 for every file.
            checksums = index.get(filename)
            if checksums == None:
                fail(
                    "no checksums for '{}' (filename '{}') in index".format(
                        nevra,
                        filename,
                    )
                )
        else:
            # Fallback for upstream repos without index – use checksums from packages.json
            checksums = pkg["checksums"]

        url = baseurl + filename
        # @oss-disable[end= ]: url = maybe_resolve_manifold_url(url)

        # Always use content based paths for member artifacts – they are
        # content-addressed via sha256.
        dl = actions.declare_output(
            nevra,
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

_download_members = dynamic_actions(
    impl = _download_members_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "index_json": dynattrs.option(dynattrs.artifact_value()),
        "members": dynattrs.dict(str, dynattrs.output()),
        "packages_jsons": dynattrs.dict(str, dynattrs.artifact_value()),
    },
)

def download_members(
    *,
    actions: AnalysisActions,
    packages_jsons: dict[str, Artifact],
    index_json: Artifact | None = None,
    baseurl: str,
    nevras: list[str],
) -> dict[str, Artifact]:
    """Download every repo member by `{name}_{version}_{arch}` using the
    checksum index.

    This is the beefed-up version of `download_packages_subtargets`: instead
    of a caller-provided list of package names, `nevras` is the full
    `members` attr (emitted by the snapshot binary) and every entry becomes
    an artifact keyed by that identifier. URLs and checksums come from the
    index files exactly like `download_packages_subtargets`. Always uses
    content based paths.
    """
    if not nevras:
        return {}

    outputs = {}
    for nevra in nevras:
        # Identifiers contain `:` (epoch separator) which is not safe in
        # output paths, so flatten it for the on-disk name. The dict keys
        # (and subtarget names) keep the real identifier.
        outputs[nevra] = actions.declare_output(
            "members",
            nevra.replace(":", "_"),
        )

    actions.dynamic_output_new(
        _download_members(
            baseurl = baseurl,
            index_json = index_json,
            packages_jsons = packages_jsons,
            members = {nevra: out.as_output() for nevra, out in outputs.items()},
        ),
    )
    return outputs
