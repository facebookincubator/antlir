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
from typing import (
    Dict, Iterable, Iterator, List, Mapping, NamedTuple, Optional, Set, Tuple
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
    RepoSnapshot,
)
from .storage import Storage
from .yum_dnf_conf import YumDnfConfRepo

# We'll download data in 512KB chunks. This needs to be reasonably large to
# avoid small-buffer overheads, but not too large, since we use `zlib` for
# incremental decompression in `parse_repodata.py`, and its API has a
# complexity bug that makes it slow for large INPUT_CHUNK/OUTPUT_CHUNK.
BUFFER_BYTES = 2 ** 19
REPOMD_MAX_RETRY_S = [2 ** i for i in range(8)]  # 256 sec ==  4m16s
RPM_MAX_RETRY_S = [2 ** i for i in range(9)]  # 512 sec ==  8m32s
log = get_file_logger(__file__)


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
    skip_non_5xx: bool = False
):
    '''Decorator used to retry a function if exceptions are thrown. `format_msg`
    should be a format string that can access any args provided to the
    decorated function. `delays` are the delays between retries, in seconds.
    `skip_non_5xx` will not retry if the exception is an HTTPError and the
    status code is not 5xx. See uses below for examples.
    '''
    def _is_exc_5xx(e: Exception):
        return (
            isinstance(e, HTTPError) and e.to_dict()['http_status'] // 100 == 5
        )

    def wrapper(fn):
        @wraps(fn)
        def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable=_is_exc_5xx if skip_non_5xx else None,
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


def _detect_mutable_rpms(
    rpm: Rpm,
    rpm_table: RpmTable,
    storage_id: str,
    all_snapshot_universes: Set[str],
    db_cfg: Dict[str, str]
):
    db_conn = DBConnectionContext.from_json(db_cfg)
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
        all_canonical_checksums = set(repo_db.get_rpm_canonical_checksums(
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
# retry if they're not 5xx errors.
@retryable('Download failed: {rpm}', RPM_MAX_RETRY_S, skip_non_5xx=True)
def _download_rpm(
    rpm: Rpm,
    repo_url: str,
    rpm_table: RpmTable,
    cfg: DownloadConfig,
) -> Tuple[str, Rpm]:
    'Returns a storage_id and a copy of `rpm` with a canonical checksum.'
    log.info(f'Downloading {rpm}')
    storage = cfg.new_storage()
    with _download_resource(repo_url, rpm.location) as input, \
            storage.writer() as output:
        # Before committing to the DB, let's standardize on one hash
        # algorithm.  Otherwise, it might happen that two repos may
        # store the same RPM hashed with different algorithms, and thus
        # trigger our "different hashes" detector for a sane RPM.
        canonical_hash = hashlib.new(CANONICAL_HASH)
        for chunk in _verify_chunk_stream(
            read_chunks(input, BUFFER_BYTES),
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

        # Remove the blob if we error before the DB commit below.
        storage_id = output.commit(remove_on_exception=True)

        db_conn = cfg.new_db_conn()
        with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
            db_storage_id = repo_db.maybe_store(rpm_table, rpm, storage_id)
            _log_if_storage_ids_differ(rpm, storage_id, db_storage_id)
            # By this point, `maybe_store` would have already asserted
            # that the stored `canonical_checksum` matches ours.  If it
            # did not, something is seriously wrong with our writer code
            # -- we should not be raising a `ReportableError` for that.
            if db_storage_id == storage_id:  # We won the race to store rpm
                repo_db.commit()  # Our `Rpm` got inserted into the DB.
            else:  # We lost the race to commit `rpm`.
                # Future: batch removes in Storage if this is slow
                storage.remove(storage_id)
            return db_storage_id, rpm


def _download_rpms(
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
    db_conn = cfg.new_db_conn()
    storage_id_to_rpm = {}
    # Download in random order to reduce collisions from racing writers.
    for rpm in shuffled(rpms):
        if not cfg.rpm_shard.in_shard(rpm):
            continue
        with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
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
                repo_db.get_rpm_storage_id_and_checksum(
                    rpm_table, rpm
                )
        # If the RPM is already stored with a matching checksum, just
        # update its `.canonical_checksum`.
        if storage_id:
            rpm = rpm._replace(canonical_checksum=canonical_checksum)
            log.debug(f'Already stored under {storage_id}: {rpm}')
        else:  # We have to download the RPM.
            try:
                storage_id, rpm = _download_rpm(
                    rpm, repo.base_url, rpm_table, cfg
                )
            # IMPORTANT: All the classes of errors that we handle below
            # have the property that we would not have stored anything
            # new in the DB, meaning that such failed RPMs will be
            # retried on the next snapshot attempt.
            except ReportableError as ex:
                # RPM checksum validation errors, scenarios where the
                # same RPM name occurs with different checksums, etc.
                storage_id = ex

        # Detect if this RPM NEVRA occurs with different contents.
        if not isinstance(storage_id, ReportableError):
            storage_id = _detect_mutable_rpms(
                rpm, rpm_table, storage_id, all_snapshot_universes, cfg.db_cfg
            )

        set_new_key(storage_id_to_rpm, storage_id, rpm)

    assert len(storage_id_to_rpm) == sum(
        cfg.rpm_shard.in_shard(r) for r in rpms
    )
    return storage_id_to_rpm


# May raise `ReportableError`s to be caught by `_download_repodatas`
def _download_repodata(
    repodata: Repodata,
    repo_url: str,
    repodata_table: RepodataTable,
    cfg: DownloadConfig,
    *,
    is_primary: bool
) -> Tuple[bool, str, Optional[List[Rpm]]]:
    '''
        - Returns True only if we just downloaded & stored this Repodata.
        - Returns our new storage_id, or the previous one from the DB.
        - For the selected primary repodata, returns a list of RPMs.
          Returns None for all others.
    '''
    db_conn = cfg.new_db_conn()
    storage = cfg.new_storage()
    # We only need to download the repodata if is not already in the DB,
    # or if it is primary (so we can parse it for RPMs).
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
        storage_id = repo_db.get_storage_id(
            repodata_table, repodata
        )

    # Nothing to do -- only need to download repodata if it's the primary
    # (so we can parse it for RPMs), or if it's not already in the DB.
    if not is_primary and storage_id:
        return (False, storage_id, None)
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
        # Must commit from inside the output context to get a storage_id.
        if outfile:
            return True, outfile.commit(), rpms
    # The repodata was already stored, and we parsed it for RPMs.
    assert storage_id is not None
    return False, storage_id, rpms


def _download_repodatas(
    repo: YumDnfConfRepo,
    repomd: RepoMetadata,
    # We mutate this dictionary on-commit to allow the caller to clean
    # up any stored repodata blobs if the download fails part-way.
    persist_storage_id_to_repodata: Mapping[str, Repodata],
    visitors: Iterable['RepoObjectVisitor'],
    cfg: DownloadConfig,
) -> Tuple[Set[Rpm], Mapping[str, Repodata]]:
    rpms = None  # We'll extract these from the primary repodata
    storage_id_to_repodata = {}  # Newly stored **and** pre-existing
    primary_repodata = pick_primary_repodata(repomd.repodatas)
    _log_size(
        f'`{repo.name}` repodata weighs',
        sum(rd.size for rd in repomd.repodatas)
    )
    # Visitors see all declared repodata, even if some downloads fail.
    for visitor in visitors:
        for repodata in repomd.repodatas:
            visitor.visit_repodata(repodata)
    repodata_table = RepodataTable()
    # Download in random order to reduce collisions from racing writers.
    for repodata in shuffled(repomd.repodatas):
        try:
            newly_stored, storage_id, maybe_rpms = _download_repodata(
                repodata,
                repo.base_url,
                repodata_table,
                cfg,
                is_primary=repodata is primary_repodata,
            )
            if newly_stored:
                set_new_key(
                    persist_storage_id_to_repodata, storage_id, repodata,
                )
            if maybe_rpms is not None:
                # Convert to a set to work around buggy repodatas, which
                # list the same RPM object twice.
                rpms = set(maybe_rpms)
        except ReportableError as ex:
            # We cannot proceed without the primary file -- raise here
            # to trigger the "top-level retry" in the snapshot driver.
            if repodata is primary_repodata:
                raise
            # This fake "storage ID" is not written to
            # `persist_storage_id_to_repodata`, so we will never attempt
            # to write it to the DB.  However, it does end up in
            # `repodata.json`, so the error is visible.
            storage_id = ex
        set_new_key(storage_id_to_repodata, storage_id, repodata)

    assert len(storage_id_to_repodata) == len(repomd.repodatas)
    assert rpms, 'Is the repo empty?'
    return rpms, storage_id_to_repodata


def _commit_repodata_and_cancel_cleanup(
    repomd: RepoMetadata,
    repo_universe: str,
    repo_name: str,
    # We'll replace our IDs by those that actually ended up in the DB
    storage_id_to_repodata: Mapping[str, Repodata],
    # Will retain only those IDs that are unused by the DB and need cleanup
    persist_storage_id_to_repodata: Mapping[str, Repodata],
    db_cfg: Dict[str, str],
):
    db_conn = DBConnectionContext.from_json(db_cfg)
    repodata_table = RepodataTable()
    with RepoDBContext(db_conn, db_conn.SQL_DIALECT) as repo_db:
        # We cannot touch `persist_storage_id_to_repodata` in the loop
        # because until the transaction commits, we must be ready to
        # delete all new storage IDs.  So instead, we will construct the
        # post-commit version of that dictionary (i.e. blobs we need to
        # delete even if the transaction lands), in this variable:
        unneeded_storage_id_to_repodata = {}
        for storage_id, repodata in persist_storage_id_to_repodata.items():
            assert not isinstance(storage_id, ReportableError), repodata
            db_storage_id = repo_db.maybe_store(
                repodata_table, repodata, storage_id
            )
            _log_if_storage_ids_differ(repodata, storage_id, db_storage_id)
            if db_storage_id != storage_id:
                set_new_key(
                    storage_id_to_repodata,
                    db_storage_id,
                    storage_id_to_repodata.pop(storage_id),
                )
                set_new_key(
                    unneeded_storage_id_to_repodata, storage_id, repodata,
                )
        repo_db.store_repomd(repo_universe, repo_name, repomd)
        repo_db.commit()
        # The DB commit was successful, and we're about to exit the
        # repo_db context, which might, at worst, raise its own error.
        # Therefore, let's prevent the `finally` cleanup from deleting
        # the blobs whose IDs we just committed to the DB.
        persist_storage_id_to_repodata.clear()
        persist_storage_id_to_repodata.update(
            unneeded_storage_id_to_repodata
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


def _download_repomds(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    cfg: DownloadConfig,
    visitors: Iterable['RepoObjectVisitor'] = (),
) -> Iterator[Tuple[YumDnfConfRepo, str, RepoMetadata]]:
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
            yield (repo, repo_universe, repomd)


def download_repos(
    repos_and_universes: Iterable[Tuple[YumDnfConfRepo, str]],
    *,
    db_cfg: Dict[str, str],
    storage_cfg: Dict[str, str],
    threads: int,
    rpm_shard: RpmShard = None,
    visitors: Iterable['RepoObjectVisitor'] = (),
) -> Iterator[Tuple[YumDnfConfRepo, RepoSnapshot]]:
    'See the top-of-file docblock.'
    if rpm_shard is None:
        rpm_shard = RpmShard(shard=0, modulo=1)  # get all RPMs
    all_snapshot_universes = frozenset(u for _, u in repos_and_universes)
    cfg = DownloadConfig(
        db_cfg=db_cfg,
        storage_cfg=storage_cfg,
        rpm_shard=rpm_shard,
        threads=threads,
    )

    # Concurrently download repomds, aggregate results
    repomd_results = _download_repomds(
        repos_and_universes, cfg, visitors
    )

    for repo, repo_universe, repomd in repomd_results:
        rpm_table = RpmTable(repo_universe)
        # ## Rationale for this cleanup logic
        #
        # For any sizable repo, the initial RPM download will be slow.
        #
        # At this point, none of the downloaded repodata is committed to the
        # DB, and all the associated blobs are still subject to
        # auto-cleanup.  The rationale is that if we fail partway through
        # the download, the repo content has likely changed and it's best to
        # redownload the metadata when we retry, rather than to persist some
        # partial and unusable metadata.
        #
        # We do two things to minimize that chances of persisting
        # partial metadata:
        #  (1) Write metadata to the DB in a single transaction.
        #  (2) Keep `remove_unneeded_storage_ids` ready to delete all
        #      newly stored (and thus unreferenced from the DB) repodata
        #      blobs, up until the moment that the transaction commits.
        persist_storage_id_to_repodata = {}
        try:
            # Download the repodata blobs to storage, and add them to
            # `persist_storage_id_to_repodata` to enable automatic cleanup on
            # error via `finally`.
            rpm_set, storage_id_to_repodata = _download_repodatas(
                repo, repomd, persist_storage_id_to_repodata, visitors, cfg,
            )

            storage_id_to_rpm = _download_rpms(
                repo, rpm_table, rpm_set, all_snapshot_universes, cfg
            )
            # Visitors inspect all RPMs, whether or not they belong to the
            # current shard.  For the RPMs in this shard, visiting after
            # `_download_rpms` allows us to pass in an `Rpm` structure
            # with `.canonical_checksum` set, to better detect identical
            # RPMs from different repos.
            for visitor in visitors:
                for rpm in {
                    **{r.location: r for r in rpm_set},
                    # Post-download Rpm objects override the pre-download ones
                    **{r.location: r for r in storage_id_to_rpm.values()},
                }.values():
                    visitor.visit_rpm(rpm)

            # Commit all the repo metadata, inactivate the `finally` cleanup
            # (except for blobs that we don't want to retain, after all.)
            _commit_repodata_and_cancel_cleanup(
                repomd,
                repo_universe,
                repo.name,
                storage_id_to_repodata,
                persist_storage_id_to_repodata,
                db_cfg
            )
        finally:
            if persist_storage_id_to_repodata:
                log.info('Deleting uncommitted blobs, do not Ctrl-C')
            storage = cfg.new_storage()
            for storage_id in persist_storage_id_to_repodata.keys():
                try:
                    storage.remove(storage_id)
                # Yes, catch even KeyboardInterrupt to minimize our litter
                except BaseException:  # pragma: no cover
                    log.exception(f'Failed to remove {storage_id}')

        yield repo, RepoSnapshot(
            repomd=repomd,
            storage_id_to_repodata=storage_id_to_repodata,
            storage_id_to_rpm=storage_id_to_rpm,
        )
