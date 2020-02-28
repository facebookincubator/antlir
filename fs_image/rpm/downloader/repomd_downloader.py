#!/usr/bin/env python3
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Tuple, Iterable, Iterator

from fs_image.common import get_file_logger
from fs_image.rpm.downloader.common import (
    DownloadConfig, DownloadResult, download_resource
)
from rpm.common import retryable
from rpm.repo_objects import RepoMetadata
from rpm.yum_dnf_conf import YumDnfConfRepo


REPOMD_MAX_RETRY_S = [2 ** i for i in range(8)]  # 256 sec ==  4m16s
log = get_file_logger(__file__)


# This should realistically only fail on HTTP errors
@retryable(
    'Download failed: {repo.name} from {repo.base_url}', REPOMD_MAX_RETRY_S
)
def _download_repomd(
    repo: YumDnfConfRepo,
    repo_universe: str,
) -> Tuple[YumDnfConfRepo, str, RepoMetadata]:
    with download_resource(
        repo.base_url, 'repodata/repomd.xml'
    ) as repomd_stream:
        repomd = RepoMetadata.new(xml=repomd_stream.read())
    return repo, repo_universe, repomd


def download_repomds(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'] = (),
) -> Iterator[DownloadResult]:
    '''Downloads all repo metadatas concurrently'''
    log.info('Downloading repomds for all repos')
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(_download_repomd, repo, repo_universe)
            for repo, repo_universe in repos_and_universes
        ]
        for future in as_completed(futures):
            repo, repo_universe, repomd = future.result()
            for visitor in visitors:
                visitor.visit_repomd(repomd)
            yield DownloadResult(
                repo=repo,
                repo_universe=repo_universe,
                repomd=repomd,
            )
