#!/usr/bin/env python3
'''
"Atomically" [1] downloads a snapshot of a single RPM repo.  Uses the
`repo_db.py` and `storage.py` abstractions to store the snapshot, while
avoiding duplication of RPMs that existed in prior snapshots.

Specifically, the user calls `RepoDownloader(...).download()`, which:

  - Downloads & parses `repomd.xml`.

  - Downloads the repodatas referenced there. Parses a primary repodata.

  - Downloads the RPMs referenced in the primary repodata.

Returns a `RepoSnapshot` containing descriptions to the stored objects.  The
dictionary keys are either "storage IDs" from the supplied `Storage` class,
or `ReportableError` instances for those that were not correctly downloaded
and stored.

[1] The snapshot is only atomic (i.e. representative of a single point in
time, as opposed to a sheared mix of the repo at various points in time) if:

  - Repodata files and RPM files are never mutated after creation. For
    repodata, this is plausible because their names include their hash.  For
    RPMs, this code includes a "mutable RPM" guard to detect files, whos
    contents changed.

  - `repomd.xml` is replaced atomically (i.e.  via `rename`) after making
    available all the new RPMs & repodatas.
'''
import hashlib
import inspect
import requests
import urllib.parse
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager, ExitStack
from functools import wraps
from io import BytesIO
from types import MappingProxyType
from typing import (
    Dict, FrozenSet, Iterable, Iterator, List, Mapping, NamedTuple, Optional,
    Set, Tuple, Union
)

from fs_image.common import get_file_logger, set_new_key, shuffled

from .common import read_chunks, retry_fn, RpmShard
from .db_connection import DBConnectionContext
from .deleted_mutable_rpms import deleted_mutable_rpms
from .open_url import open_url
from .parse_repodata import get_rpm_parser, pick_primary_repodata
from .repo_objects import CANONICAL_HASH, Checksum, Repodata, RepoMetadata, Rpm
from .repo_db import RepoDBContext, RepodataTable, RpmTable
from .repo_snapshot import (
    FileIntegrityError, HTTPError, MutableRpmError, ReportableError,
    RepoSnapshot, MaybeStorageID
)
from .storage import Storage
from .yum_dnf_conf import YumDnfConfRepo

# We'll download data in 512KB chunks. This needs to be reasonably large to
# avoid small-buffer overheads, but not too large, since we use `zlib` for
# incremental decompression in `parse_repodata.py`, and its API has a
# complexity bug that makes it slow for large INPUT_CHUNK/OUTPUT_CHUNK.
BUFFER_BYTES = 2 ** 19
REPOMD_MAX_RETRY_S = [2 ** i for i in range(8)]  # 256 sec ==  4m16s
REPODATA_MAX_RETRY_S = [2 ** i for i in range(10)]  # 1024sec == 17m4s
RPM_MAX_RETRY_S = [2 ** i for i in range(9)]  # 512 sec ==  8m32s
log = get_file_logger(__file__)
RepodataReturnType = Tuple[Repodata, bool, MaybeStorageID, Optional[List[Rpm]]]


class RepodataParseError(Exception):
    pass


# Lightweight configuration used by various parts of the download
class DownloadConfig(NamedTuple):
    db_cfg: Dict[str, str]
    storage_cfg: Dict[str, str]
    rpm_shard: RpmShard
    threads: int

    def new_db_conn(self):
        return DBConnectionContext.from_json(self.db_cfg)

    def new_storage(self):
        return Storage.from_json(self.storage_cfg)


# Gets incrementally populated throughout repo downloading; used to carry info
# through the concurrent downloads until the final repo snapshot is built
class DownloadResult(NamedTuple):
    repo: YumDnfConfRepo
    repo_universe: str
    repomd: RepoMetadata
    # Below 3 params are populated incrementally after the initial 3
    storage_id_to_repodata: Optional[Mapping[MaybeStorageID, Repodata]] = None
    storage_id_to_rpm: Optional[Mapping[MaybeStorageID, Rpm]] = None
    rpms: Optional[FrozenSet[Rpm]] = None


def _verify_chunk_stream(
    chunks: Iterable[bytes], checksums: Iterable[Checksum], size: int,
    location: str,
):
    actual_size = 0
    hashers = [ck.hasher() for ck in checksums]
    for chunk in chunks:
        actual_size += len(chunk)
        for hasher in hashers:
            hasher.update(chunk)
        yield chunk
    if actual_size != size:
        raise FileIntegrityError(
            location=location,
            failed_check='size',
            expected=size,
            actual=actual_size,
        )
    for hash, ck in zip(hashers, checksums):
        if hash.hexdigest() != ck.hexdigest:
            raise FileIntegrityError(
                location=location,
                failed_check=ck.algorithm,
                expected=ck.hexdigest,
                actual=hash.hexdigest(),
            )


def _log_if_storage_ids_differ(obj, storage_id, db_storage_id):
    if db_storage_id != storage_id:
        log.warning(
            f'Another writer already committed {obj} at {db_storage_id}, '
            f'will delete our copy at {storage_id}'
        )


def _log_size(what_str: str, total_bytes: int):
    log.info(f'{what_str} {total_bytes/10**9:,.4f} GB')


def retryable(
    format_msg: str,
    delays: Iterable[float],
    *,
    skip_expected_http_errors: bool = False
):
    '''Decorator used to retry a function if exceptions are thrown. `format_msg`
    should be a format string that can access any args provided to the
    decorated function. `delays` are the delays between retries, in seconds.
    `skip_expected_http_errors` will not retry if the exception is an HTTPError
    and the status code is not either 5xx or 408. See uses below for examples.
    '''
    def _is_e_skippable(e: Exception):
        if not isinstance(e, HTTPError):
            return False
        # 408 is 'Request Timeout' and, as with 5xx, can reasonably be presumed
        # to be a transient issue that's worth retrying
        status = e.to_dict()['http_status']
        return status // 100 == 5 or status == 408

    def wrapper(fn):
        @wraps(fn)
        def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable=(
                    _is_e_skippable if skip_expected_http_errors else None
                ),
                delays=delays,
                what=format_msg.format(**fn_args)
            )
        return decorated
    return wrapper


@contextmanager
def _download_resource(repo_url: str, relative_url: str) -> Iterator[BytesIO]:
    if not repo_url.endswith('/'):
        repo_url += '/'  # `urljoin` needs a trailing / to work right
    assert not relative_url.startswith('/')
    try:
        with open_url(
            urllib.parse.urljoin(repo_url, relative_url)
        ) as input:
            yield input
    except requests.exceptions.HTTPError as ex:
        # E.g. we can see 404 errors if packages were deleted
        # without updating the repodata.
        #
        # Future: If we see lots of transient error status codes
        # in practice, we could retry automatically before
        # waiting for the next snapshot, but the complexity is
        # not worth it for now.
        raise HTTPError(
            location=relative_url,
            http_status=ex.response.status_code,
        )


# Note that we use this function serially from the master thread after
# performing the downloads. This is because it's possible for SQLite to run
# into locking issues with many concurrent writers. Additionally, these writes
# are a minor portion of our overall execution time and thus we see negligible
# perf gains by parallelizing them.
def _maybe_write_id(
    repo_obj: Union[Repodata, Rpm],
    storage_id: MaybeStorageID,
    table: RepodataTable,
    db_ctx: RepoDBContext,
):
    '''Used to write a storage_id to repo_db after a possible download.'''
    # Don't store errors into the repo db
    if isinstance(storage_id, ReportableError):
        return storage_id
    with db_ctx as repo_db_ctx:
        db_storage_id = repo_db_ctx.maybe_store(table, repo_obj, storage_id)
        repo_db_ctx.commit()
    _log_if_storage_ids_differ(repo_obj, storage_id, db_storage_id)
    return db_storage_id


def _detect_mutable_rpms(
    rpm: Rpm,
    rpm_table: RpmTable,
    storage_id: str,
    all_snapshot_universes: Set[str],
    db_ctx: RepoDBContext,
) -> MaybeStorageID:
    with db_ctx as repo_db_ctx:
        all_canonical_checksums = set(repo_db_ctx.get_rpm_canonical_checksums(
            rpm_table, rpm, all_snapshot_universes,
        ))
    assert all_canonical_checksums, (rpm, storage_id)
    assert all(
        c.algorithm == CANONICAL_HASH for c in all_canonical_checksums
    ), all_canonical_checksums
    all_canonical_checksums.remove(rpm.canonical_checksum)
    deleted_checksums = deleted_mutable_rpms.get(rpm.nevra(), set())
    assert rpm.canonical_checksum not in deleted_checksums, \
        f'{rpm} was in deleted_mutable_rpms, but still exists in repos'
    all_canonical_checksums.difference_update(deleted_checksums)
    if all_canonical_checksums:
        # Future: It would be nice to mark all mentions of the NEVRA
        # as bad, but that requires messy updates of multiple
        # `RepoSnapshot`s.  For now, we rely on the fact that the next
        # `snapshot-repos` run will do this anyway.
        return MutableRpmError(
            location=rpm.location,
            storage_id=storage_id,
            checksum=rpm.canonical_checksum,
            other_checksums=all_canonical_checksums,
        )
    return storage_id


# May raise `ReportableError`s to be caught by `_download_rpms`.
# May raise an `HTTPError` if the download fails, which won't trigger a
# retry if they're not 5xx/408 errors.
@retryable(
    'Download failed: {rpm}', RPM_MAX_RETRY_S, skip_expected_http_errors=True
)
def _download_rpm(
    rpm: Rpm,
    repo_url: str,
    rpm_table: RpmTable,
    cfg: DownloadConfig,
) -> Tuple[Rpm, str]:
    'Returns a storage_id and a copy of `rpm` with a canonical checksum.'
    log.info(f'Downloading {rpm}')
    storage = cfg.new_storage()
    with _download_resource(repo_url, rpm.location) as input_, \
            storage.writer() as output:
        # Before committing to the DB, let's standardize on one hash
        # algorithm.  Otherwise, it might happen that two repos may
        # store the same RPM hashed with different algorithms, and thus
        # trigger our "different hashes" detector for a sane RPM.
        canonical_hash = hashlib.new(CANONICAL_HASH)
        for chunk in _verify_chunk_stream(
            read_chunks(input_, BUFFER_BYTES),
            [rpm.checksum],
            rpm.size,
            rpm.location,
        ):  # May raise a ReportableError
            canonical_hash.update(chunk)
            output.write(chunk)
        # NB: We can also query the RPM as we download it above, via
        # something like P123285392.  However, at present, all necessary
        # metadata can be retrieved via `parse_metadata.py`.
        rpm = rpm._replace(canonical_checksum=Checksum(
            algorithm=CANONICAL_HASH, hexdigest=canonical_hash.hexdigest(),
        ))
        storage_id = output.commit()
    assert storage_id is not None
    return rpm, storage_id


def _handle_rpm(
    rpm: Rpm,
    repo_url: str,
    rpm_table: RpmTable,
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
) -> Tuple[Rpm, MaybeStorageID]:
    db_conn = cfg.new_db_conn()
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db_ctx:
        # If we get no `storage_id` back, there are 3 possibilities:
        #  - `rpm.nevra()` was never seen before.
        #  - `rpm.nevra()` was seen before, but it was hashed with
        #     different algorithm(s), so we MUST download and
        #     compute the canonical checksum to know if its contents
        #     are the same.
        #  - `rpm.nevra()` was seen before, **AND** one of the
        #    prior checksums used `rpm.checksum.algorithms`, but
        #    produced a different hash value.  In other words, this
        #    is a `MutableRpmError`, because the same NEVRA must
        #    have had two different contents.  We COULD explicitly
        #    detect this error here, and avoid the download.
        #    However, this severe error should be infrequent, and we
        #    actually get valuable information from the download --
        #    this lets us know whether the file is wrong or the
        #    repodata is wrong.
        storage_id, canonical_checksum = \
            repo_db_ctx.get_rpm_storage_id_and_checksum(rpm_table, rpm)
    # If the RPM is already stored with a matching checksum, just update its
    # `.canonical_checksum`.
    if storage_id:
        rpm = rpm._replace(canonical_checksum=canonical_checksum)
        # This is a very common case and thus noisy log, so we write to debug
        log.debug(f'Already stored under {storage_id}: {rpm}')
        return rpm, storage_id
    # We have to download the RPM.
    try:
        return _download_rpm(rpm, repo_url, rpm_table, cfg)
    # RPM checksum validation errors, HTTP errors, etc
    except ReportableError as ex:
        # This "fake" storage_id is stored in `storage_id_to_rpm`, so the error
        # is propagated to sqlite db through the snapshot. It isn't written to
        # repo_db however as that happens in the *_impl function
        return rpm, ex


def _download_rpms_threaded(
    repo: YumDnfConfRepo,
    rpm_table: RpmTable,
    rpms: Iterable[Rpm],
    all_snapshot_universes: Set[str],
    cfg: DownloadConfig,
):
    _log_size(
        f'`{repo.name}` has {len(rpms)} RPMs weighing',
        sum(r.size for r in rpms)
    )
    storage_id_to_rpm = {}
    db_conn = cfg.new_db_conn()
    db_ctx = RepoDBContext(db_conn, db_conn.SQL_DIALECT)
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = [
            executor.submit(
                _handle_rpm,
                rpm,
                repo.base_url,
                rpm_table,
                all_snapshot_universes,
                cfg,
            )
            # Download in random order to reduce collisions from racing writers.
            for rpm in shuffled(rpms)
            if cfg.rpm_shard.in_shard(rpm)
        ]
        for future in as_completed(futures):
            rpm, res_storage_id = future.result()
            # If it's valid, we store this storage_id to repo_db regardless of
            # whether we encounter fatal errors later on in the execution and
            # don't finish the snapshot - see top-level docblock for reasoning
            storage_id_or_err = _maybe_write_id(
                rpm, res_storage_id, rpm_table, db_ctx
            )
            # Detect if this RPM NEVRA occurs with different contents.
            if not isinstance(storage_id_or_err, ReportableError):
                storage_id_or_err = _detect_mutable_rpms(
                    rpm, rpm_table, storage_id_or_err, all_snapshot_universes,
                    db_ctx
                )
            set_new_key(storage_id_to_rpm, storage_id_or_err, rpm)

    assert len(storage_id_to_rpm) == sum(
        cfg.rpm_shard.in_shard(r) for r in rpms
    )
    return storage_id_to_rpm


def _get_rpms_from_repodatas(
    repodata_results: Iterable[DownloadResult],
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'],
    all_snapshot_universes: FrozenSet[str],
) -> Iterator[DownloadResult]:
    for res in repodata_results:
        storage_id_to_rpm = _download_rpms_threaded(
            res.repo,
            RpmTable(res.repo_universe),
            res.rpms,
            all_snapshot_universes,
            cfg,
        )
        # Visitors inspect all RPMs, whether or not they belong to the
        # current shard.  For the RPMs in this shard, visiting after
        # `_download_rpms` allows us to pass in an `Rpm` structure
        # with `.canonical_checksum` set, to better detect identical
        # RPMs from different repos.
        for visitor in visitors:
            for rpm in {
                **{r.location: r for r in res.rpms},
                # Post-download Rpm objects override the pre-download ones
                **{r.location: r for r in storage_id_to_rpm.values()},
            }.values():
                visitor.visit_rpm(rpm)
        yield res._replace(
            storage_id_to_rpm=MappingProxyType(storage_id_to_rpm)
        )


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
                _download_resource(repo_url, repodata.location)
            )
            # Want to persist the downloaded repodata into storage so that
            # future runs don't need to redownload it
            outfile = cm.enter_context(storage.writer())

        log.info(f'Fetching {repodata}')
        for chunk in _verify_chunk_stream(
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


def _download_repodatas_threaded(
    repo: YumDnfConfRepo,
    repomd: RepoMetadata,
    visitors: Iterable['RepoObjectVisitor'],
    cfg: DownloadConfig,
) -> Tuple[Set[Rpm], Mapping[str, Repodata]]:
    rpms = None  # We'll extract these from the primary repodata
    storage_id_to_repodata = {}  # Newly stored **and** pre-existing
    repodata_table = RepodataTable()
    primary_repodata = pick_primary_repodata(repomd.repodatas)
    _log_size(
        f'`{repo.name}` repodata weighs',
        sum(rd.size for rd in repomd.repodatas)
    )
    # Visitors see all declared repodata, even if some downloads fail.
    for visitor in visitors:
        for repodata in repomd.repodatas:
            visitor.visit_repodata(repodata)
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
                storage_id_or_err = _maybe_write_id(
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


def _get_repodatas_from_repomds(
    repomd_results: Iterable[DownloadResult],
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'],
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
        rpm_set, storage_id_to_repodata = _download_repodatas_threaded(
            res.repo, res.repomd, visitors, cfg,
        )
        yield res._replace(
            storage_id_to_repodata=MappingProxyType(storage_id_to_repodata),
            rpms=rpm_set,
        )


# This should realistically only fail on HTTP errors
@retryable(
    'Download failed: {repo.name} from {repo.base_url}', REPOMD_MAX_RETRY_S
)
def _download_repomd(
    repo: YumDnfConfRepo,
    repo_universe: str,
) -> Tuple[YumDnfConfRepo, str, RepoMetadata]:
    with _download_resource(
        repo.base_url, 'repodata/repomd.xml'
    ) as repomd_stream:
        repomd = RepoMetadata.new(xml=repomd_stream.read())
    return repo, repo_universe, repomd


def _download_repomds_threaded(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'] = (),
) -> Iterator[DownloadResult]:
    '''Downloads all repo metadatas concurrently'''
    with ThreadPoolExecutor(max_workers=cfg.threads) as executor:
        futures = []
        for repo, repo_universe in repos_and_universes:
            log.info(f'Downloading repo {repo.name} from {repo.base_url}')
            futures.append(
                executor.submit(_download_repomd, repo, repo_universe)
            )
        for future in as_completed(futures):
            repo, repo_universe, repomd = future.result()
            for visitor in visitors:
                visitor.visit_repomd(repomd)
            yield DownloadResult(
                repo=repo,
                repo_universe=repo_universe,
                repomd=repomd,
            )


def download_repos(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    *,
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'] = (),
) -> Iterator[Tuple[YumDnfConfRepo, RepoSnapshot]]:
    'See the top-of-file docblock.'
    all_snapshot_universes = frozenset(u for _, u in repos_and_universes)

    # Concurrently download repomds, aggregate results
    repomd_results = _download_repomds_threaded(
        repos_and_universes, cfg, visitors
    )
    repodata_results = _get_repodatas_from_repomds(
        repomd_results, cfg, visitors
    )
    # Cast to run the generators before storing into the db
    rpm_results = list(_get_rpms_from_repodatas(
        repodata_results, cfg, visitors, all_snapshot_universes
    ))

    # All downloads have completed - we now want to atomically persist repomds.
    db_conn = cfg.new_db_conn()
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
        # Even though a valid snapshot of a single repo is intrinsically valid,
        # we only want to operate on coherent collections of repos (as they
        # existed at roughly the same point in time). For this reason, we'd
        # rather leak already-committed repodata & RPM objects (subject to GC
        # later, if we choose) if we were not able to store a full snapshot,
        # while not doing so for repomds (as committing those essentially
        # commits a full snasphot, given that the repodata & RPM objects will
        # now be referenced).
        for res in rpm_results:
            repo_db.store_repomd(
                res.repo_universe, res.repo.name, res.repomd
            )
        try:
            repo_db.commit()
        except Exception:  # pragma: no cover
            # This is bad, but we hope this commit was atomic and thus none of
            # the repomds got inserted, in which case our snapshot's failed but
            # we at least don't have a semi-complete snapshot in the db.
            log.exception(f'Exception when trying to commit repomd')
            raise

    return (
        (
            res.repo,
            RepoSnapshot(
                repomd=res.repomd,
                storage_id_to_repodata=res.storage_id_to_repodata,
                storage_id_to_rpm=res.storage_id_to_rpm,
            )
        )
        for res in rpm_results
    )
