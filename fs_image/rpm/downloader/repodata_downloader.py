#!/usr/bin/env python3
from contextlib import ExitStack
from concurrent.futures import ThreadPoolExecutor, as_completed
from types import MappingProxyType
from typing import (
    Tuple, Iterable, Iterator, List, Mapping, Optional, Set
)

from fs_image.common import get_file_logger, set_new_key, shuffled
from fs_image.rpm.downloader.common import (
    BUFFER_BYTES, DownloadConfig, DownloadResult, download_resource, log_size,
    maybe_write_id, verify_chunk_stream
)
from rpm.common import read_chunks, retryable
from rpm.parse_repodata import get_rpm_parser, pick_primary_repodata
from rpm.repo_db import RepoDBContext, RepodataTable
from rpm.repo_objects import RepoMetadata, Repodata, Rpm
from rpm.repo_snapshot import MaybeStorageID, ReportableError
from rpm.yum_dnf_conf import YumDnfConfRepo


REPODATA_MAX_RETRY_S = [2 ** i for i in range(10)]  # 1024sec == 17m4s
log = get_file_logger(__file__)
RepodataReturnType = Tuple[Repodata, bool, MaybeStorageID, Optional[List[Rpm]]]


class RepodataParseError(Exception):
    pass


# May raise `ReportableError`s to be caught by `_download_repodatas`
def _download_repodata_impl(
    repodata: Repodata,
    *,
    repo_url: str,
    repodata_table: RepodataTable,
    cfg: DownloadConfig,
    is_primary: bool
) -> RepodataReturnType:
    '''This function behaves differently depending on two main characteristics:
      - Whether or not the provided repodata is primary, and
      - Whether or not it already exists in storage
    Which actions are taken depends on which of the above true, and this
    branching is explained within the function.
    '''
    db_conn = cfg.new_db_conn()
    storage = cfg.new_storage()
    # We only need to download the repodata if is not already in the DB,
    # or if it is primary (so we can parse it for RPMs).
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db_ctx:
        storage_id = repo_db_ctx.get_storage_id(repodata_table, repodata)

    # Nothing to do -- only need to download repodata if it's the primary
    # (so we can parse it for RPMs), or if it's not already in the DB.
    if not is_primary and storage_id:
        return repodata, False, storage_id, None
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

        log.info(f'Fetching {repodata}')
        for chunk in verify_chunk_stream(
            read_chunks(infile, BUFFER_BYTES),
            [repodata.checksum],
            repodata.size,
            repodata.location,
        ):  # May raise a ReportableError
            if outfile:
                outfile.write(chunk)
            if rpm_parser:
                try:
                    rpms.extend(rpm_parser.feed(chunk))
                except Exception as ex:
                    # Not a ReportableError so it won't trigger a retry
                    raise RepodataParseError((repodata.location, ex))
        # Must commit the output context to get a storage_id.
        if outfile:
            return repodata, True, outfile.commit(), rpms
    # The primary repodata was already stored, and we just parsed it for RPMs.
    assert storage_id is not None
    return repodata, False, storage_id, rpms


@retryable(
    'Download failed: repodata at {repodata.location}',
    REPODATA_MAX_RETRY_S
)
def _download_repodata(
    repodata, is_primary: bool, **kwargs
) -> RepodataReturnType:
    '''Wrapper to handle ReportableError and force a retry if we're trying
    to retrieve the primary.

    RepodataReturnType is a 3-tuple with the following properties:
        - [0]: The repodata that was operated on
        - [1]: A new storage_id (if it was just downloaded), an existing
                storage_id if it was already in the db, or an error if this was
                a non-primary repodata and a ReportableError was raised.
        - [2]: List of RPMs if it was primary repodata, else None.
    '''
    try:
        return _download_repodata_impl(
            repodata, **kwargs, is_primary=is_primary
        )
    except ReportableError as e:
        # Can't proceed without primary file; raise to trigger retry.
        if is_primary:
            raise e
        # This "fake" storage_id is stored in `storage_id_to_repodata`, so the
        # error is propagated to the sqlite db through the repo snapshot. It
        # isn't written to repo_db however as that explicitly skip errors.
        return repodata, False, e, None


def _download_repodatas(
    repo: YumDnfConfRepo,
    repomd: RepoMetadata,
    cfg: DownloadConfig,
) -> Tuple[Set[Rpm], Mapping[str, Repodata]]:
    rpms = None  # We'll extract these from the primary repodata
    storage_id_to_repodata = {}  # Newly stored **and** pre-existing
    repodata_table = RepodataTable()
    primary_repodata = pick_primary_repodata(repomd.repodatas)
    log_size(
        f'`{repo.name}` repodata weighs',
        sum(rd.size for rd in repomd.repodatas)
    )
    db_conn = cfg.new_db_conn()
    db_ctx = RepoDBContext(db_conn, db_conn.SQL_DIALECT)
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(
                _download_repodata,
                repodata,
                repo_url=repo.base_url,
                repodata_table=repodata_table,
                cfg=cfg,
                is_primary=repodata is primary_repodata
            )
            for repodata in shuffled(repomd.repodatas)
        ]

        for future in as_completed(futures):
            repodata, newly_stored, storage_id_or_err, maybe_rpms = \
                future.result()
            if newly_stored:
                # This repodata was newly downloaded and stored in storage, so
                # we store its storage_id to repo_db regardless of whether we
                # encounter fatal errors later on in the execution and don't
                # finish the snapshot - see top-level docblock for reasoning
                storage_id_or_err = maybe_write_id(
                    repodata, storage_id_or_err, repodata_table, db_ctx
                )
            if maybe_rpms is not None:
                # RPMs will only have been returned by the primary, thus we
                # should only enter this block once
                assert rpms is None
                # Convert to a set to work around buggy repodatas, which
                # list the same RPM object twice.
                rpms = frozenset(maybe_rpms)
            set_new_key(
                storage_id_to_repodata, storage_id_or_err, repodata
            )
    # It's possible that for non-primary repodatas we received errors when
    # downloading - in that case we store the error in the sqlite db, thus the
    # dict should contain an entry for every single repodata
    assert len(storage_id_to_repodata) == len(repomd.repodatas)
    assert rpms, 'Is the repo empty?'
    return rpms, storage_id_to_repodata


def gen_repodatas_from_repomds(
    repomd_results: Iterable[DownloadResult],
    cfg: DownloadConfig,
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
            res.repo, res.repomd, cfg,
        )
        yield res._replace(
            storage_id_to_repodata=MappingProxyType(storage_id_to_repodata),
            rpms=rpm_set,
        )
