# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("//antlir/antlir2/package_managers/snapshot/facebook:manifold.bzl", "maybe_resolve_manifold_url")

def download(
        *,
        actions: AnalysisActions,
        url: str,
        metadata_run_info: RunInfo | None = None,
        out_name: str | None = None,
        out: OutputArtifact | None = None,
        checksums: dict[str, str] = {},
        allow_nondeterministic_downloads: bool = False) -> Artifact:
    # @oss-disable[end= ]: url = maybe_resolve_manifold_url(url)

    if not allow_nondeterministic_downloads and not checksums:
        fail("when allow_nondeterministic_downloads=False, at least one checksum must be provided")

    filetype = url.rsplit(".", 1)[-1]
    if filetype not in ("xz", "gz"):
        filetype = None

    has_content_based_path = False
    digest_config = actions.digest_config()
    if digest_config.allows_sha1() and "sha1" in checksums:
        has_content_based_path = True
    if digest_config.allows_sha256() and "sha256" in checksums:
        has_content_based_path = True
    if not checksums and allow_nondeterministic_downloads:
        has_content_based_path = True

    if not out:
        out = actions.declare_output(
            out_name,
            has_content_based_path = has_content_based_path,
        )

    if filetype:
        dl_target = actions.declare_output(
            out.short_path + "." + filetype,
            has_content_based_path = has_content_based_path,
        )
    else:
        dl_target = out

    if checksums:
        actions.download_file(
            dl_target,
            url,
            sha256 = checksums.get("sha256", None),
            sha1 = checksums.get("sha1", None),
        )
    if not checksums and allow_nondeterministic_downloads:
        # It sucks to have any nondeterministic actions, but sometimes we need
        # to get things up and running super quickly and we need a
        # non-deterministic entrypoint (for example if we are downloading
        # upstrema artifacts that may change but give us deterministic data
        # after that initial entrypoint)
        actions.run(
            cmd_args(
                "curl",
                "-L",
                url,
                "-o",
                dl_target.as_output(),
            ),
            env = {"https_proxy": "fwdproxy:8080"},
            category = "curl_nondeterministic",
            identifier = out.short_path,
            # Must be able to hit network
            local_only = True,
            # Everything after this is hopefully well-behaved and deterministic,
            # but we don't want to cache this nondeterministic artifact
            allow_cache_upload = False,
        )

    if filetype:
        if not metadata_run_info:
            fail("metadata_run_info is required to decompress {} files".format(filetype))
        actions.run(
            cmd_args(
                metadata_run_info,
                "decompress",
                dl_target,
                out,
                cmd_args(filetype, format = "--format={}"),
            ),
            category = "decompress",
            identifier = out.short_path,
        )

    if isinstance(out, OutputArtifact):
        return out.as_input()
    return out
