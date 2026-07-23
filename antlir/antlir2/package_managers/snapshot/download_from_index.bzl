# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/package_managers/snapshot/facebook:manifold.bzl", "maybe_resolve_manifold_url")
load(":content_based_path.bzl", "should_use_content_based_path")

def _download_from_index_impl(
    actions: AnalysisActions,
    index_json: ArtifactValue,
    baseurl: str,
    files: dict[str, typing.Any],
):
    index = index_json.read_json()

    for relpath, out in files.items():
        checksums = index.get(relpath)
        if checksums == None:
            fail("no checksums for relpath found '{}' in index {}".format(relpath, index_json))

        url = baseurl + relpath
        # @oss-disable[end= ]: url = maybe_resolve_manifold_url(url)

        has_content_based_path = should_use_content_based_path(actions, checksums)
        dl = actions.declare_output(
            relpath,
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

_download_from_index = dynamic_actions(
    impl = _download_from_index_impl,
    attrs = {
        "baseurl": dynattrs.value(str),
        "files": dynattrs.dict(str, dynattrs.output()),
        "index_json": dynattrs.artifact_value(),
    },
)

def download_from_index(
    *,
    actions: AnalysisActions,
    index_json: Artifact,
    baseurl: str,
    relpaths: list[str],
) -> dict[str, Artifact]:
    """Download files listed in an index.json that maps relpath → checksums.

    The index JSON is expected to be a dict of relpath → {"sha1":..., "sha256":...}.
    For each relpath in relpaths, this will declare an output and download it
    via actions.download_file using checksums from the index.
    """
    if not relpaths:
        return {}

    outputs = {}
    for relpath in relpaths:
        outputs[relpath] = actions.declare_output(relpath)

    actions.dynamic_output_new(
        _download_from_index(
            baseurl = baseurl,
            index_json = index_json,
            files = {relpath: out.as_output() for relpath, out in outputs.items()},
        ),
    )
    return outputs
