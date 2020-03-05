#!/usr/bin/env python3
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Iterable, Iterator, Tuple

from fs_image.common import get_file_logger
from fs_image.rpm.downloader.common import (
    DownloadConfig,
    DownloadResult,
    download_resource,
)
from rpm.common import retryable
from rpm.repo_objects import RepoMetadata
from rpm.yum_dnf_conf import YumDnfConfRepo


REPOMD_MAX_RETRY_S = [2 ** i for i in range(8)]  # 256 sec ==  4m16s
LOOP_LIMIT = 5  # Times we'll loop downloading repomds before exiting
log = get_file_logger(__file__)


# This should realistically only fail on HTTP errors
@retryable("Download failed: {repo.name} from {repo.base_url}", REPOMD_MAX_RETRY_S)
def _download_repomd(
    repo: YumDnfConfRepo, repo_universe: str
) -> Tuple[YumDnfConfRepo, str, RepoMetadata]:
    with download_resource(repo.base_url, "repodata/repomd.xml") as repomd_stream:
        repomd = RepoMetadata.new(xml=repomd_stream.read())
    return repo, repo_universe, repomd


def _download_repomds(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]], cfg: DownloadConfig
) -> Iterator[DownloadResult]:
    """Downloads all repo metadatas concurrently"""
    log.info("Downloading repomds for all repos")
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(_download_repomd, repo, repo_universe)
            for repo, repo_universe in repos_and_universes
        ]
        for future in as_completed(futures):
            repo, repo_universe, repomd = future.result()
            yield DownloadResult(repo=repo, repo_universe=repo_universe, repomd=repomd)


def gen_repomds_from_repos(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]], cfg: DownloadConfig
) -> Iterator[DownloadResult]:
    # Concurrently download repomds and aggregate results
    repomd_results = list(_download_repomds(repos_and_universes, cfg))
    # Perform the repomd download at least twice in a row, and ensure that the
    # checksums from the two downloads match up. This gives us added protection
    # against the scenario where a repo object wasn't atomically moved between
    # repos.
    #
    # We arbitrarily limit the iterations to ensure we don't get stuck looping
    # infinitely if there's an underlying integrity issue.
    for _ in range(LOOP_LIMIT):
        prev_repomd_results = repomd_results
        repomd_results = list(_download_repomds(repos_and_universes, cfg))
        if sorted(res.repomd.checksum for res in prev_repomd_results) == sorted(
            res.repomd.checksum for res in repomd_results
        ):
            break
    else:
        # We hit our loop limit, so there's likely an integrity issue to fix
        log.critical(
            "Failed to download repomd because each successive download "
            "produced a different set of repomds. This indicates an integrity "
            "issue with the repos."
        )
        raise RuntimeError("Integrity issue with repos")
    yield from repomd_results
