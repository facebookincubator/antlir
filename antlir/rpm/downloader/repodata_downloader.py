#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from concurrent.futures import as_completed, ThreadPoolExecutor
from contextlib import ExitStack
from types import MappingProxyType
from typing import (
    FrozenSet,
    Iterable,
    Iterator,
    List,
    Mapping,
    NamedTuple,
    Optional,
    Tuple,
)

from antlir.common import get_logger, retryable, set_new_key, shuffled
from antlir.rpm.common import read_chunks
from antlir.rpm.downloader.common import (
    BUFFER_BYTES,
    download_resource,
    DownloadConfig,
    DownloadResult,
    log_size,
    maybe_write_id,
    verify_chunk_stream,
)
from antlir.rpm.parse_repodata import get_rpm_parser, pick_primary_repodata
from antlir.rpm.repo_db import RepodataTable
from antlir.rpm.repo_objects import Repodata, RepoMetadata, Rpm
from antlir.rpm.repo_snapshot import ReportableError
from antlir.rpm.yum_dnf_conf import YumDnfConfRepo


REPODATA_MAX_RETRY_S = [2**i for i in range(10)]  # 1024sec == 17m4s
log = get_logger()


class RepodataParseError(Exception):
    pass


class DownloadRepodataReturnType(NamedTuple):
    # The repodata that was operated on
    repodata: Repodata
    # True if the repodata was stored into storage on this run, else False
    newly_stored: bool
    # A new storage_id (if it was just downloaded), or an existing storage_id if
    # it was already in the db.
    storage_id: str
    # List of RPMs if it was primary repodata, else None.
    maybe_rpms: Optional[List[Rpm]]


# May raise `ReportableError`, which will abort the snapshot
@retryable(
    "Download failed: repodata at {repodata.location}", REPODATA_MAX_RETRY_S
)
def _download_repodata(
    repodata: Repodata,
    *,
    repo_url: str,
    repodata_table: RepodataTable,
    cfg: DownloadConfig,
    is_primary: bool,
) -> DownloadRepodataReturnType:
    """This function behaves differently depending on two main characteristics:
      - Whether or not the provided repodata is primary, and
      - Whether or not it already exists in storage.
    Which actions are taken depends on which of the above true, and this
    branching is explained within the function.
    """
    storage = cfg.new_storage()
    # We only need to download the repodata if is not already in the DB,
    # or if it is primary (so we can parse it for RPMs).
    with cfg.new_db_ctx(readonly=True) as ro_repo_db:
        storage_id = ro_repo_db.get_storage_id(repodata_table, repodata)

    # Nothing to do -- only need to download repodata if it's the primary
    # (so we can parse it for RPMs), or if it's not already in the DB.
    if not is_primary and storage_id:
        return DownloadRepodataReturnType(repodata, False, storage_id, None)
    rpms = [] if is_primary else None

    # Remaining possibilities are that we've got a primary with or without
    # a storage_id, or a non-primary without a storage_id
    with ExitStack() as cm:
        rpm_parser = None
        if is_primary:
            # We'll parse the selected primary file to discover the RPMs.
            rpm_parser = cm.enter_context(get_rpm_parser(repodata))

        if storage_id:
            # Read the primary from storage as we already have an ID
            infile = cm.enter_context(storage.reader(storage_id))
            # No need to write as this repodata was already stored
            outfile = None
        else:
            # Nothing stored, must download - can fail due to repo updates
            infile = cm.enter_context(
                download_resource(repo_url, repodata.location)
            )
            # Want to persist the downloaded repodata into storage so that
            # future runs don't need to redownload it
            outfile = cm.enter_context(storage.writer())

        log.info(f"Fetching {repodata} from {repo_url}")
        for chunk in verify_chunk_stream(
            read_chunks(infile, BUFFER_BYTES),
            [repodata.checksum],
            repodata.size,
            repodata.location,
        ):  # May raise a ReportableError
            if outfile:
                outfile.write(chunk)
            if rpm_parser:
                assert rpms is not None
                try:
                    rpms.extend(rpm_parser.feed(chunk))
                except Exception as ex:
                    raise RepodataParseError((repodata.location, ex))
        # We consider `source_rpm` mandatory, since it's the best source of
        # RPM grouping information that we have available.
        if rpms:
            no_source_rpm = [
                r
                for r in rpms
                if r.source_rpm is None and not r.location.endswith(".src.rpm")
            ]
            assert not no_source_rpm, f"Empty source_rpm: {no_source_rpm}"
        # Must commit the output context to get a storage_id.
        if outfile:
            return DownloadRepodataReturnType(
                repodata, True, outfile.commit(), rpms
            )
    # The primary repodata was already stored, and we just parsed it for RPMs.
    assert storage_id is not None
    return DownloadRepodataReturnType(repodata, False, storage_id, rpms)


def _download_repodatas(
    repo: YumDnfConfRepo, repomd: RepoMetadata, cfg: DownloadConfig
) -> Tuple[FrozenSet[Rpm], Mapping[str, Repodata]]:
    rpms = None  # We'll extract these from the primary repodata
    storage_id_to_repodata = {}  # Newly stored **and** pre-existing
    repodata_table = RepodataTable()
    primary_repodata = pick_primary_repodata(repomd.repodatas)
    log_size(
        f"`{repo.name}` repodata weighs",
        sum(rd.size for rd in repomd.repodatas),
    )
    rw_db_conn = cfg.new_db_conn(readonly=False)
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(
                _download_repodata,
                repodata,
                repo_url=repo.base_url,
                repodata_table=repodata_table,
                cfg=cfg,
                is_primary=repodata is primary_repodata,
            )
            for repodata in shuffled(repomd.repodatas)
        ]

        for future in as_completed(futures):
            res = future.result()
            if res.newly_stored:
                # Don't want to store errors into the repo db -- this should
                # never be the case as `newly_stored` is only True when we
                # successfully commit a new repodata to storage
                assert not isinstance(res.storage_id, ReportableError)
                # This repodata was newly downloaded and stored in storage, so
                # we store its storage_id to repo_db regardless of whether we
                # encounter fatal errors later on that fail the snapshot; see
                # docblock in `repo_downloader.py` for reasoning
                storage_id = maybe_write_id(
                    res.repodata, res.storage_id, repodata_table, rw_db_conn
                )
            else:
                storage_id = res.storage_id
            if res.maybe_rpms is not None:
                # RPMs will only have been returned by the primary, thus we
                # should only enter this block once
                assert rpms is None
                # Convert to a set to work around buggy repodatas, which
                # list the same RPM object twice.
                rpms = frozenset(res.maybe_rpms)
            set_new_key(storage_id_to_repodata, storage_id, res.repodata)
    # It's possible that for non-primary repodatas we received errors when
    # downloading - in that case we store the error in the sqlite db, thus the
    # dict should contain an entry for every single repodata
    assert len(storage_id_to_repodata) == len(repomd.repodatas)
    if not rpms:
        log.warning(f"Repo {repo} has no RPMs")
        rpms = frozenset()
    return rpms, storage_id_to_repodata


def gen_repodatas_from_repomds(
    repomd_results: Iterable[DownloadResult], cfg: DownloadConfig
) -> Iterator[DownloadResult]:
    # For each downloaded repomd, concurrently download the contained
    # repodatas. This driver runs serially, but each repomd's repodatas are
    # downloaded in parallel. We could run this driver concurrently as well,
    # but we're likely already saturating the network with the downloads for a
    # single repomd and won't see a perf increase from further parallelization.
    for res in repomd_results:
        # We explicitly omit any complex clean-up logic here, and store
        # repodatas regardless of whether they end up actually being used (i.e.
        # their referencing repomd gets committed).
        #
        # The main reason for this is that the cost we pay to store these
        # dangling repodatas is fairly negligible when compared to the size of
        # the overall repos, and if we ever run into issues of these extra
        # objects taking up too much space, we can easily add a periodic job to
        # scan the db and remove any unused references. We are also able to
        # avoid implementing a lot of complex cleanup logic this way.
        rpm_set, storage_id_to_repodata = _download_repodatas(
            res.repo, res.repomd, cfg
        )
        yield res._replace(
            storage_id_to_repodata=MappingProxyType(storage_id_to_repodata),
            rpms=rpm_set,
        )
